// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hidden building blocks for the RSJS descriptor compilation.
//!
//! This module intentionally does not drive the normal `proa build` command.
//! It provides the rustc-wrapper half of a future two-compilation pipeline and
//! a strict collector for the Cargo JSON produced by that pipeline.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use rsjs_ir::abi::{CargoUnitId, InvocationId, PlanId, SemanticHash};
use rsjs_ir::build::{ArtifactRole, LinkArtifact, LinkUnitDependency, LinkUnitManifest};

/// Enables the hidden descriptor rustc-wrapper dispatch.
pub const WRAPPER_MODE_ENV: &str = "PROA_RSJS_RUSTC_WRAPPER_MODE";
/// The only wrapper protocol accepted by this version of the CLI.
pub const WRAPPER_MODE_DESCRIPTOR_V1: &str = "descriptor-v1";
/// Final-compilation wrapper protocol that restores descriptor Cargo-unit IDs.
pub const WRAPPER_MODE_FINAL_V2: &str = "final-v2";
/// Version of the wrapper-owned final rustc argument/environment policy.
///
/// This must change whenever wrapper behavior can affect rustc output without
/// changing the exact plan or unit-map bytes.
pub const FINAL_WRAPPER_POLICY_VERSION: u16 = 8;
/// Filesystem/output policy for persistent descriptor Cargo targets.
///
/// This is deliberately independent of sidecar schema: bump it whenever a
/// previous target may contain wrapper/proc-macro artifacts that must never
/// participate in current build identity.
pub const DESCRIPTOR_TARGET_POLICY_VERSION: u16 = 7;
/// Explicit host-local staging directory for descriptor objects and sidecars.
pub const DESCRIPTOR_STAGING_DIR_ENV: &str = "PROA_RSJS_DESCRIPTOR_STAGING_DIR";
/// Exact runner-owned Cargo target directory for descriptor compilation.
pub const DESCRIPTOR_TARGET_DIR_ENV: &str = "PROA_RSJS_DESCRIPTOR_TARGET_DIR";
/// Exact generated subtree excluded from package-source closure hashing in
/// both descriptor and final compilation.
pub const SOURCE_EXCLUSION_DIR_ENV: &str = "PROA_RSJS_SOURCE_EXCLUSION_DIR";
/// Exact invocation- and wrapper-policy-scoped final target owned by the
/// final Cargo runner.
pub const FINAL_TARGET_DIR_ENV: &str = "PROA_RSJS_FINAL_TARGET_DIR";
/// Legacy proc-macro filesystem descriptor fallback; not identity-safe.
pub const COMPONENT_DESCRIPTOR_DIRS_ENV: &str = "RSJS_COMPONENT_DESCRIPTOR_DIRS";
pub(crate) const DESCRIPTOR_TARGET_MARKER_FILE: &str = ".proa-rsjs-descriptor-target-v1";
pub(crate) const DESCRIPTOR_TARGET_MARKER_BYTES: &[u8] = b"proa-rsjs-descriptor-target-v1\n";
pub(crate) const DESCRIPTOR_STAGING_MARKER_FILE: &str = ".proa-rsjs-descriptor-staging-v1";
pub(crate) const DESCRIPTOR_STAGING_MARKER_BYTES: &[u8] = b"proa-rsjs-descriptor-staging-v1\n";
const DESCRIPTOR_RECOVERY_ATTEMPTS_DIRECTORY: &str = ".proa-rsjs-recovery-attempts";
const DESCRIPTOR_RECOVERY_ATTEMPT_PREFIX: &str = "attempt-";
/// Exact content-addressed descriptor-to-final unit map.
pub const FINAL_UNIT_MAP_PATH_ENV: &str = "PROA_RSJS_FINAL_UNIT_MAP_PATH";
/// SHA-256 of the exact final unit-map bytes.
pub const FINAL_UNIT_MAP_HASH_ENV: &str = "PROA_RSJS_FINAL_UNIT_MAP_HASH";
/// Stable, lowercase-hex Cargo-unit identity exposed to RSJS proc macros.
pub const CARGO_UNIT_ID_ENV: &str = "RSJS_CARGO_UNIT_ID";
/// Explicit final-wrapper result visible to proc macros.
pub const FINAL_CARGO_UNIT_STATUS_ENV: &str = "RSJS_FINAL_CARGO_UNIT_STATUS";
/// The final rustc invocation matched exactly one descriptor unit.
pub const FINAL_CARGO_UNIT_STATUS_MAPPED: &str = "mapped-v1";
/// No descriptor unit matched; an RSJS macro must fail rather than guess.
pub const FINAL_CARGO_UNIT_STATUS_UNMAPPED: &str = "unmapped-v1";
/// Marks rustc as running the descriptor phase rather than final compilation.
pub const DESCRIPTOR_PHASE_ENV: &str = "RSJS_DESCRIPTOR_PHASE";
/// Schema version of [`DescriptorUnitSidecar`].
pub const DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION: u16 = 11;
/// Schema version of [`FinalUnitMap`].
pub const FINAL_UNIT_MAP_SCHEMA_VERSION: u16 = 7;
pub(crate) const FINAL_UNIT_INPUT_ENV_PREFIX: &str = "PROA_RSJS_FINAL_UNIT_INPUT_";
pub(crate) const FINAL_UNIT_PLAN_ENV_PREFIX: &str = "PROA_RSJS_FINAL_UNIT_PLAN_";
pub(crate) const FINAL_UNIT_PLAN_DIRECTORY_ENV: &str = "PROA_RSJS_FINAL_UNIT_PLAN_DIRECTORY";
const FINAL_UNIT_MAP_FORMAT: &str = "proa.rsjs.final_unit_map";
/// Marks rustc as running the linked final phase.
pub const FINAL_PHASE_ENV: &str = "RSJS_FINAL_PHASE";
/// Exact content-addressed link-plan path consumed by final macro expansion.
pub const FINAL_PLAN_PATH_ENV: &str = "RSJS_LINK_PLAN_PATH";
/// Lower-case hexadecimal strong plan identity.
pub const FINAL_PLAN_ID_ENV: &str = "RSJS_PLAN_ID";
/// Tagged SHA-256 digest of the exact linked-plan bytes.
pub const FINAL_PLAN_FILE_HASH_ENV: &str = "RSJS_LINK_PLAN_FILE_HASH";
const UNIT_HASH_DOMAIN: &[u8] = b"proa.rsjs.descriptor-cargo-unit.v3";
const PROFILE_HASH_DOMAIN: &[u8] = b"proa.rsjs.final-match-profile.v1";
const FILE_HASH_DOMAIN: &[u8] = b"proa.rsjs.descriptor-input-file.v1";
const PACKAGE_SOURCE_CLOSURE_HASH_DOMAIN: &[u8] = b"proa.rsjs.package-source-closure.v4";
const PACKAGE_SOURCE_FILE_HASH_DOMAIN: &[u8] = b"proa.rsjs.package-source-file.v1";
const OUT_DIR_FILE_HASH_DOMAIN: &[u8] = b"proa.rsjs.out-dir-file.v1";
const OBSERVED_SOURCE_FILE_HASH_DOMAIN: &[u8] = b"proa.rsjs.observed-source-file.v1";
const OBSERVED_ENVIRONMENT_VALUE_HASH_DOMAIN: &[u8] = b"proa.rsjs.observed-environment-value.v1";
const LOGICAL_ENVIRONMENT_HASH_DOMAIN: &[u8] = b"proa.rsjs.logical-environment.v2";
const REALIZED_ENVIRONMENT_HASH_DOMAIN: &[u8] = b"proa.rsjs.realized-rustc-environment.v1";
pub(crate) const DESCRIPTOR_OBJECT_HASH_DOMAIN: &[u8] = b"proa.rsjs.descriptor-object.v1";
const LINK_ARTIFACT_HASH_DOMAIN: &[u8] = b"proa.rsjs.link-artifact.v1";
const DESCRIPTOR_CODEGEN_POLICY: [&str; 5] = [
    "codegen-units=cargo-explicit-or-incremental-64",
    "incremental=cargo",
    "link-dead-code=yes",
    "lto=off",
    "debuginfo=0",
];
const DESCRIPTOR_STRIP_POLICY: &str = "strip=none";
const DESCRIPTOR_INCREMENTAL_CODEGEN_UNITS: u32 = 64;

static ATOMIC_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

/// Return whether the process environment requests hidden wrapper dispatch.
pub fn rustc_wrapper_dispatch_requested() -> bool {
    env::var_os(WRAPPER_MODE_ENV).is_some()
}

/// Run one invocation received through Cargo's `RUSTC_WRAPPER` protocol.
///
/// Cargo invokes a wrapper as `wrapper rustc rustc-args...`. Probe invocations
/// are forwarded byte-for-byte. Compilation invocations additionally emit one
/// complete descriptor carrier and one atomic unit sidecar.
pub fn run_rustc_wrapper_from_env<I>(args: I) -> Result<ExitStatus, WrapperError>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mode = env::var(WRAPPER_MODE_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(WRAPPER_MODE_ENV))?;
    if mode == super::descriptor_objects::LINKER_MODE {
        return super::descriptor_objects::run_linker(args.get(1..).unwrap_or_default())
            .map_err(|error| WrapperError::DescriptorCarrier(error.to_string()));
    }
    reject_composed_workspace_wrapper(&args)?;
    match mode.as_str() {
        WRAPPER_MODE_DESCRIPTOR_V1 => {
            let staging_dir = env::var_os(DESCRIPTOR_STAGING_DIR_ENV)
                .map(PathBuf::from)
                .ok_or(WrapperError::MissingEnvironment(DESCRIPTOR_STAGING_DIR_ENV))?;
            if !staging_dir.is_absolute() {
                return Err(WrapperError::RelativeStagingDirectory(staging_dir));
            }
            let target_dir = env::var_os(DESCRIPTOR_TARGET_DIR_ENV)
                .map(PathBuf::from)
                .ok_or(WrapperError::MissingEnvironment(DESCRIPTOR_TARGET_DIR_ENV))?;
            if !target_dir.is_absolute() {
                return Err(WrapperError::RelativeDescriptorTargetDirectory(target_dir));
            }
            let source_exclusion = env::var_os(SOURCE_EXCLUSION_DIR_ENV)
                .map(PathBuf::from)
                .ok_or(WrapperError::MissingEnvironment(SOURCE_EXCLUSION_DIR_ENV))?;
            if !source_exclusion.is_absolute() {
                return Err(WrapperError::RelativeSourceExclusionDirectory(
                    source_exclusion,
                ));
            }
            run_descriptor_rustc_wrapper(args, &staging_dir, &target_dir, &source_exclusion)
        }
        WRAPPER_MODE_FINAL_V2 => run_final_rustc_wrapper(args),
        _ => Err(WrapperError::UnsupportedWrapperMode(mode)),
    }
}

/// Cargo composes `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` as
/// `outer-wrapper workspace-wrapper rustc rustc-args...`. Cargo configuration
/// does not necessarily export the latter as an environment variable, so the
/// runner's environment checks alone cannot detect this unsupported shape.
fn reject_composed_workspace_wrapper(args: &[OsString]) -> Result<(), WrapperError> {
    let Some(candidate_rustc_leader) = args.get(3) else {
        return Ok(());
    };
    let Some(candidate_workspace_wrapper) = args.get(1) else {
        return Ok(());
    };
    let Some(candidate_rustc) = args.get(2) else {
        return Ok(());
    };

    if !is_cargo_rustc_argument_leader(candidate_rustc)
        && is_cargo_rustc_argument_leader(candidate_rustc_leader)
    {
        return Err(WrapperError::ComposedWorkspaceWrapper {
            wrapper: candidate_workspace_wrapper.clone(),
            rustc: candidate_rustc.clone(),
        });
    }
    Ok(())
}

fn is_cargo_rustc_argument_leader(argument: &OsStr) -> bool {
    argument.to_str().is_some_and(|argument| {
        argument == "-"
            || matches!(
                argument,
                "-vV" | "-V" | "--version" | "--help" | "-h" | "--crate-name"
            )
            || argument.starts_with("--crate-name=")
            || argument == "--print"
            || argument.starts_with("--print=")
    })
}

fn run_descriptor_rustc_wrapper<I>(
    args: I,
    staging_dir: &Path,
    target_dir: &Path,
    source_exclusion: &Path,
) -> Result<ExitStatus, WrapperError>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let rustc = args
        .get(1)
        .ok_or(WrapperError::MissingRustcProgram)?
        .clone();
    let rustc_args = args.get(2..).unwrap_or_default();

    if is_rustc_probe(rustc_args) {
        return wrapped_rustc_command(&rustc)
            .args(rustc_args)
            .status()
            .map_err(|source| WrapperError::SpawnRustc {
                program: PathBuf::from(rustc),
                source,
            });
    }

    let rustc_identity = query_rustc_identity(&rustc)?;
    let environment = rustc_visible_environment()?;
    let cargo_manifest_dir = match env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        Some(directory) if !directory.is_absolute() => {
            return Err(WrapperError::RelativeCargoManifestDirectory(directory));
        }
        directory => directory,
    };
    let out_dir =
        descriptor_out_dir_coordinate(target_dir, environment.get("OUT_DIR").map(String::as_str))?;
    let input = normalize_rustc_invocation_with_package_root(
        rustc_args,
        &environment,
        &rustc_identity,
        cargo_manifest_dir.as_deref(),
        Some(source_exclusion),
    )?;
    let cargo_extra_filename =
        input
            .cargo_extra_filename()
            .map_err(|reason| WrapperError::InvalidCargoExtraFilename {
                context: "descriptor rustc invocation",
                value: input
                    .codegen_options
                    .get("extra-filename")
                    .cloned()
                    .unwrap_or_default(),
                reason,
            })?;
    let cargo_unit_id = input.cargo_unit_id();
    let cargo_unit_id_hex = cargo_unit_id.to_string();
    let dep_info_path = rustc_dep_info_path(rustc_args, &input)?;

    let objects_dir = staging_dir.join("objects");
    let units_dir = staging_dir.join("units");
    fs::create_dir_all(&objects_dir).map_err(|source| WrapperError::CreateStagingDirectory {
        path: objects_dir.clone(),
        source,
    })?;
    fs::create_dir_all(&units_dir).map_err(|source| WrapperError::CreateStagingDirectory {
        path: units_dir.clone(),
        source,
    })?;

    let object_file_name = format!("{cargo_unit_id_hex}.o");
    let object_path = objects_dir.join(&object_file_name);
    let carrier = descriptor_carrier_mode(&input, &dep_info_path, rustc_args)?;
    let forwarded_args = descriptor_rustc_args_with_carrier(rustc_args, &object_path, &carrier)?;
    validate_dep_info_output_path(target_dir, &dep_info_path)?;
    prepare_dep_info_output(&dep_info_path)?;

    let mut command = wrapped_rustc_command(&rustc);
    command
        .args(&forwarded_args)
        .env(CARGO_UNIT_ID_ENV, &cargo_unit_id_hex)
        .env(DESCRIPTOR_PHASE_ENV, "1");
    if let DescriptorCarrierMode::Native(linker) = &carrier {
        command
            .env(WRAPPER_MODE_ENV, super::descriptor_objects::LINKER_MODE)
            .env(super::descriptor_objects::LINKER_PROGRAM_ENV, linker)
            .env(super::descriptor_objects::LINKER_CARRIER_ENV, &object_path);
    }
    let status = command
        .status()
        .map_err(|source| WrapperError::SpawnRustc {
            program: PathBuf::from(&rustc),
            source,
        })?;

    if !status.success() {
        return Ok(status);
    }
    if let DescriptorCarrierMode::Rlib(path) = &carrier {
        fs::copy(path, &object_path).map_err(|source| WrapperError::HashInput {
            path: path.clone(),
            source,
        })?;
    }
    if !object_path.is_file() {
        return Err(WrapperError::MissingDescriptorObject(object_path));
    }

    let dep_info_working_directory =
        canonical_plain_directory(&env::current_dir().map_err(WrapperError::CurrentDirectory)?)?;
    let mut descriptor_phase_environment = BTreeMap::new();
    descriptor_phase_environment.insert(
        CARGO_UNIT_ID_ENV.to_string(),
        Some(cargo_unit_id_hex.clone()),
    );
    descriptor_phase_environment.insert(DESCRIPTOR_PHASE_ENV.to_string(), Some("1".to_string()));
    let observed = observe_rustc_inputs(
        &dep_info_path,
        &dep_info_working_directory,
        environment.get("OUT_DIR").map(Path::new),
        out_dir.as_deref(),
        &descriptor_phase_environment,
    )?;
    let dep_info_coordinate = target_relative_dep_info_path(target_dir, &dep_info_path)?;

    let descriptor_object_hash =
        hash_file(&object_path, DESCRIPTOR_OBJECT_HASH_DOMAIN).map_err(|source| {
            WrapperError::HashInput {
                path: object_path.clone(),
                source,
            }
        })?;
    let sidecar = DescriptorUnitSidecar {
        schema_version: DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
        cargo_unit_id: cargo_unit_id_hex.clone(),
        cargo_manifest_path: cargo_manifest_dir.map(|directory| directory.join("Cargo.toml")),
        cargo_package_name: env::var("CARGO_PKG_NAME").ok(),
        cargo_package_version: env::var("CARGO_PKG_VERSION").ok(),
        cargo_profile: env::var("PROFILE").ok(),
        cargo_profile_fingerprint: hex_digest(&input.final_match_profile_hash()),
        cargo_primary_package: env::var_os("CARGO_PRIMARY_PACKAGE").is_some(),
        cargo_extra_filename,
        crate_name: input.crate_name.clone(),
        crate_types: input.crate_types.iter().cloned().collect(),
        target: input.target.clone(),
        cfgs: input.cfgs.iter().cloned().collect(),
        features: input.features.iter().cloned().collect(),
        source_hash: hex_digest(&input.source_hash),
        package_source_closure_hash: hex_digest(&input.package_source_closure_hash),
        dependency_artifacts: input
            .dependencies
            .iter()
            .map(|(name, digest)| {
                let path = input
                    .dependency_paths
                    .get(name)
                    .expect("every normalized dependency retains its exact input path");
                (
                    name.clone(),
                    DescriptorDependencyArtifact {
                        path: path.clone(),
                        content_hash: hex_digest(digest),
                    },
                )
            })
            .collect(),
        normalized_rustc_inputs_hash: hex_digest(&input.normalized_inputs_hash()),
        logical_environment_hash: hex_digest(&input.logical_environment_hash()),
        dep_info_path: dep_info_coordinate,
        dep_info_working_directory,
        out_dir,
        out_dir_dependencies: observed.out_dir_dependencies,
        source_dependencies: observed.source_dependencies,
        environment_dependencies: observed.environment_dependencies,
        realized_environment_hash: hex_digest(&realized_environment_hash(&environment)),
        descriptor_codegen: DescriptorCodegenSidecar {
            codegen_units: if matches!(carrier, DescriptorCarrierMode::Single) {
                1
            } else {
                descriptor_codegen_units(rustc_args)?
            },
            incremental: rustc_codegen_value(rustc_args, "incremental")?.is_some(),
            link_dead_code: true,
            lto: "off".to_string(),
            strip: "none".to_string(),
        },
        descriptor_object: format!("objects/{object_file_name}"),
        descriptor_object_hash: hex_digest(&descriptor_object_hash),
    };
    let sidecar_bytes =
        serde_json::to_vec_pretty(&sidecar).map_err(WrapperError::SerializeUnitSidecar)?;
    let sidecar_path = units_dir.join(format!("{cargo_unit_id_hex}.json"));
    atomic_write(&sidecar_path, &sidecar_bytes)?;

    Ok(status)
}

fn run_final_rustc_wrapper<I>(args: I) -> Result<ExitStatus, WrapperError>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let rustc = args
        .get(1)
        .ok_or(WrapperError::MissingRustcProgram)?
        .clone();
    let rustc_args = args.get(2..).unwrap_or_default();

    if is_rustc_probe(rustc_args) {
        return wrapped_rustc_command(&rustc)
            .args(rustc_args)
            .status()
            .map_err(|source| WrapperError::SpawnRustc {
                program: PathBuf::from(rustc),
                source,
            });
    }

    let unit_map = load_final_unit_map_from_env()?;
    let final_target = env::var_os(FINAL_TARGET_DIR_ENV)
        .map(PathBuf::from)
        .ok_or(WrapperError::MissingEnvironment(FINAL_TARGET_DIR_ENV))?;
    let final_out_dir_raw = env::var_os("OUT_DIR").map(PathBuf::from);
    let final_out_dir = validate_final_out_dir(&final_target, final_out_dir_raw.as_deref())?;
    let final_out_dir_coordinate =
        final_out_dir_coordinate(&final_target, final_out_dir_raw.as_deref())?;
    // Derive the join coordinate from Cargo's untouched invocation first.
    // Plan-sensitive RSJS expansions emit built-in `env!` dependencies for
    // the plan and unit-map fingerprints. Cargo therefore invalidates those
    // units before this wrapper runs while retaining unrelated artifacts in
    // the stable final target. Keeping the invariant final cfgs out of Cargo's
    // RUSTFLAGS preserves Cargo's metadata/extra-filename discriminator across
    // the descriptor and final phases.
    let invocation = final_unit_coordinate_from_invocation(rustc_args)?;
    let final_plan_args = final_plan_rustc_args_from_env()?;
    let final_child_environment = final_child_environment_from_env()?;
    let mapped = unit_map.resolve(invocation.as_ref().map(|value| &value.coordinate));
    let final_visible_environment = rustc_visible_environment()?;
    let mut final_phase_environment = BTreeMap::<String, Option<String>>::new();
    for (name, value) in &final_child_environment {
        let value = value
            .to_str()
            .ok_or(WrapperError::NonUtf8PhaseOwnedEnvironment(name))?;
        final_phase_environment.insert((*name).to_string(), Some(value.to_string()));
    }
    if let Some(mapped) = mapped {
        final_phase_environment.insert(
            CARGO_UNIT_ID_ENV.to_string(),
            Some(mapped.cargo_unit_id.to_string()),
        );
        final_phase_environment.insert(
            FINAL_CARGO_UNIT_STATUS_ENV.to_string(),
            Some(FINAL_CARGO_UNIT_STATUS_MAPPED.to_string()),
        );
    } else {
        final_phase_environment.insert(
            FINAL_CARGO_UNIT_STATUS_ENV.to_string(),
            Some(FINAL_CARGO_UNIT_STATUS_UNMAPPED.to_string()),
        );
    }
    let dep_info_path = invocation
        .as_ref()
        .map(|value| rustc_dep_info_path(rustc_args, &value.input))
        .transpose()?;
    if let Some(mapped) = mapped {
        validate_current_out_dir_dependencies(
            final_out_dir.as_deref(),
            &mapped.out_dir_dependencies,
        )?;
        validate_current_source_dependencies(&mapped.source_dependencies)?;
        validate_current_environment_dependencies(
            &final_visible_environment,
            final_out_dir_raw.as_deref(),
            final_out_dir_coordinate.as_deref(),
            &mapped.environment_dependencies,
        )?;
    }
    if let Some(dep_info_path) = dep_info_path.as_deref() {
        validate_dep_info_output_path(&final_target, dep_info_path)?;
        prepare_dep_info_output(dep_info_path)?;
    }

    if let Some(mapped) = mapped {
        let (name, expected) = &mapped.tracked_input;
        if env::var(name).as_deref() != Ok(expected.as_str()) {
            return Err(WrapperError::DescriptorCarrier(format!(
                "final unit tracking input {name} is missing or stale"
            )));
        }
        final_phase_environment.insert(name.clone(), Some(expected.clone()));
    }
    let projection_environment = match mapped {
        Some(mapped) => final_unit_projection_environment(mapped)?,
        None => BTreeMap::new(),
    };
    for (name, value) in &projection_environment {
        final_phase_environment.insert(name.clone(), Some(value.clone()));
    }
    let mut command = wrapped_rustc_command(&rustc);
    command.envs(&projection_environment);
    if let Some(mapped) = mapped {
        command.env(&mapped.tracked_input.0, &mapped.tracked_input.1);
    }
    command
        .args(rustc_args)
        .args(final_plan_args)
        .envs(&final_child_environment)
        .env_remove(DESCRIPTOR_PHASE_ENV)
        .env_remove(CARGO_UNIT_ID_ENV);
    if let Some(mapped) = mapped {
        command
            .env(CARGO_UNIT_ID_ENV, mapped.cargo_unit_id.to_string())
            .env(FINAL_CARGO_UNIT_STATUS_ENV, FINAL_CARGO_UNIT_STATUS_MAPPED);
    } else {
        command.env(
            FINAL_CARGO_UNIT_STATUS_ENV,
            FINAL_CARGO_UNIT_STATUS_UNMAPPED,
        );
    }
    let status = command
        .status()
        .map_err(|source| WrapperError::SpawnRustc {
            program: PathBuf::from(rustc),
            source,
        })?;
    if status.success() {
        if let (Some(mapped), Some(dep_info_path)) = (mapped, dep_info_path.as_deref()) {
            let working_directory = canonical_plain_directory(
                &env::current_dir().map_err(WrapperError::CurrentDirectory)?,
            )?;
            let actual = observe_rustc_inputs(
                dep_info_path,
                &working_directory,
                final_out_dir_raw.as_deref(),
                final_out_dir_coordinate.as_deref(),
                &final_phase_environment,
            )?;
            if actual.out_dir_dependencies != mapped.out_dir_dependencies {
                return Err(WrapperError::FinalOutDirDependencySetMismatch {
                    expected: mapped.out_dir_dependencies.clone(),
                    actual: actual.out_dir_dependencies,
                });
            }
            if actual.source_dependencies != mapped.source_dependencies
                && !source_dependency_sets_equivalent(
                    &mapped.source_dependencies,
                    &actual.source_dependencies,
                )
            {
                return Err(WrapperError::FinalSourceDependencySetMismatch {
                    expected: mapped.source_dependencies.clone(),
                    actual: actual.source_dependencies,
                });
            }
            if actual.environment_dependencies != mapped.environment_dependencies {
                return Err(WrapperError::FinalEnvironmentDependencySetMismatch {
                    expected: mapped.environment_dependencies.clone(),
                    actual: actual.environment_dependencies,
                });
            }
        }
    }
    Ok(status)
}

const FINAL_CHILD_ENVIRONMENT: [&str; 5] = [
    FINAL_PHASE_ENV,
    FINAL_PLAN_PATH_ENV,
    FINAL_PLAN_ID_ENV,
    FINAL_PLAN_FILE_HASH_ENV,
    FINAL_UNIT_MAP_HASH_ENV,
];

fn final_child_environment_from_env() -> Result<BTreeMap<&'static str, OsString>, WrapperError> {
    let mut environment = BTreeMap::new();
    for name in FINAL_CHILD_ENVIRONMENT {
        let value = env::var_os(name).ok_or(WrapperError::MissingEnvironment(name))?;
        environment.insert(name, value);
    }
    if environment.get(FINAL_PHASE_ENV).map(OsString::as_os_str) != Some(OsStr::new("1")) {
        return Err(WrapperError::InvalidFinalPhase);
    }
    let plan_path = PathBuf::from(
        environment
            .get(FINAL_PLAN_PATH_ENV)
            .expect("the required final plan path was collected")
            .as_os_str(),
    );
    if !plan_path.is_absolute() {
        return Err(WrapperError::RelativeFinalPlan(plan_path));
    }
    Ok(environment)
}

fn final_unit_projection_environment(
    mapped: &LoadedFinalUnit,
) -> Result<BTreeMap<String, String>, WrapperError> {
    let Some(directory) = env::var_os(FINAL_UNIT_PLAN_DIRECTORY_ENV) else {
        // Legacy transport-only runners retain the global plan witnesses.
        return Ok(BTreeMap::new());
    };
    let directory = PathBuf::from(directory);
    let name = format!("{FINAL_UNIT_PLAN_ENV_PREFIX}{}", mapped.cargo_unit_id);
    let hash = env::var(&name).map_err(|_| {
        WrapperError::DescriptorCarrier(format!("missing final unit projection input {name}"))
    })?;
    validate_lower_sha256(&hash).map_err(|reason| {
        WrapperError::DescriptorCarrier(format!(
            "invalid final unit projection input {name}: {reason}"
        ))
    })?;
    let path = directory.join(format!(
        "{hash}.{}",
        rsjs_ir::final_plan::UNIT_PROJECTION_FILE_SUFFIX
    ));
    let exclusion = env::var_os(SOURCE_EXCLUSION_DIR_ENV)
        .map(PathBuf::from)
        .ok_or(WrapperError::MissingEnvironment(SOURCE_EXCLUSION_DIR_ENV))?;
    super::final_compile::ensure_plain_managed_file(&exclusion, &path)
        .map_err(|error| WrapperError::DescriptorCarrier(error.to_string()))?;
    let bytes = fs::read(&path).map_err(|error| {
        WrapperError::DescriptorCarrier(format!("failed to read final unit projection: {error}"))
    })?;
    if exact_sha256_hex(&bytes) != hash {
        return Err(WrapperError::DescriptorCarrier(
            "final unit projection bytes do not match their content hash".to_owned(),
        ));
    }
    let directory = directory
        .to_str()
        .ok_or(WrapperError::NonUtf8PhaseOwnedEnvironment(
            FINAL_UNIT_PLAN_DIRECTORY_ENV,
        ))?;
    Ok(BTreeMap::from([
        (name, hash),
        (
            FINAL_UNIT_PLAN_DIRECTORY_ENV.to_owned(),
            directory.to_owned(),
        ),
    ]))
}

fn final_plan_rustc_args_from_env() -> Result<Vec<OsString>, WrapperError> {
    let plan_id = env::var(FINAL_PLAN_ID_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_PLAN_ID_ENV))?;
    validate_lower_sha256(&plan_id).map_err(|reason| WrapperError::InvalidSha256 {
        name: FINAL_PLAN_ID_ENV,
        value: plan_id.clone(),
        reason,
    })?;
    let plan_file_hash = env::var(FINAL_PLAN_FILE_HASH_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_PLAN_FILE_HASH_ENV))?;
    validate_tagged_sha256(&plan_file_hash).map_err(|reason| WrapperError::InvalidSha256 {
        name: FINAL_PLAN_FILE_HASH_ENV,
        value: plan_file_hash.clone(),
        reason,
    })?;
    let unit_map_hash = env::var(FINAL_UNIT_MAP_HASH_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_UNIT_MAP_HASH_ENV))?;
    validate_lower_sha256(&unit_map_hash).map_err(|reason| WrapperError::InvalidSha256 {
        name: FINAL_UNIT_MAP_HASH_ENV,
        value: unit_map_hash.clone(),
        reason,
    })?;

    Ok(final_plan_rustc_args(
        &plan_id,
        &plan_file_hash,
        &unit_map_hash,
    ))
}

fn final_plan_rustc_args(
    _plan_id: &str,
    _plan_file_hash: &str,
    _unit_map_hash: &str,
) -> Vec<OsString> {
    [
        "--cfg".to_string(),
        "rsjs_final".to_string(),
        "--cfg".to_string(),
        format!("rsjs_final_wrapper_policy=\"{FINAL_WRAPPER_POLICY_VERSION}\""),
        "--check-cfg".to_string(),
        "cfg(rsjs_final)".to_string(),
        "--check-cfg".to_string(),
        format!("cfg(rsjs_final_wrapper_policy,values(\"{FINAL_WRAPPER_POLICY_VERSION}\"))"),
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn wrapped_rustc_command(rustc: &OsStr) -> Command {
    let mut command = Command::new(rustc);
    scrub_wrapper_control_environment(&mut command);
    command
}

fn is_rustc_probe(args: &[OsString]) -> bool {
    if args.len() == 1 {
        let argument = args[0].to_string_lossy();
        if matches!(
            argument.as_ref(),
            "-vV" | "-V" | "--version" | "--help" | "-h"
        ) {
            return true;
        }
    }

    let mut prints = Vec::new();
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].to_string_lossy();
        if argument == "--emit" || argument.starts_with("--emit=") {
            return false;
        }
        if argument == "--print" {
            index += 1;
            let Some(value) = args.get(index).and_then(|value| value.to_str()) else {
                return false;
            };
            prints.push(value.to_string());
        } else if let Some(value) = argument.strip_prefix("--print=") {
            prints.push(value.to_string());
        } else if rustc_probe_option_consumes_next(&argument) {
            index += 1;
            if args.get(index).is_none() {
                return false;
            }
        } else if argument == "-" || !argument.starts_with('-') {
            positional.push(argument.to_string());
        }
        index += 1;
    }

    !prints.is_empty()
        && prints.iter().all(|value| {
            matches!(
                value.split('=').next().unwrap_or_default(),
                "calling-conventions"
                    | "all-target-specs-json"
                    | "cfg"
                    | "check-cfg"
                    | "code-models"
                    | "crate-name"
                    | "deployment-target"
                    | "file-names"
                    | "host-tuple"
                    | "relocation-models"
                    | "split-debuginfo"
                    | "stack-protector-strategies"
                    | "sysroot"
                    | "target-cpus"
                    | "target-features"
                    | "target-libdir"
                    | "target-list"
                    | "target-spec-json"
                    | "target-spec-json-schema"
                    | "tls-models"
            )
        })
        && positional.iter().all(|argument| argument == "-")
}

fn rustc_probe_option_consumes_next(argument: &str) -> bool {
    matches!(
        argument,
        "--cap-lints"
            | "--cfg"
            | "--check-cfg"
            | "--color"
            | "--crate-attr"
            | "--crate-name"
            | "--crate-type"
            | "--diagnostic-width"
            | "--edition"
            | "--error-format"
            | "--extern"
            | "--json"
            | "--out-dir"
            | "--remap-path-prefix"
            | "--remap-path-scope"
            | "--sysroot"
            | "--target"
            | "-C"
            | "-L"
            | "-Z"
            | "-l"
            | "-o"
    )
}

fn query_rustc_identity(rustc: &OsStr) -> Result<Vec<u8>, WrapperError> {
    let output =
        Command::new(rustc)
            .arg("-vV")
            .output()
            .map_err(|source| WrapperError::SpawnRustc {
                program: PathBuf::from(rustc),
                source,
            })?;
    if !output.status.success() {
        return Err(WrapperError::RustcIdentityFailed {
            program: PathBuf::from(rustc),
            status: output.status,
        });
    }
    Ok(output.stdout)
}

fn rustc_visible_environment() -> Result<BTreeMap<String, String>, WrapperError> {
    let mut environment = BTreeMap::new();
    for (name, value) in env::vars_os() {
        let name = name
            .into_string()
            .map_err(WrapperError::NonUtf8EnvironmentName)?;
        if wrapper_control_environment(&name) {
            continue;
        }
        let value = value
            .into_string()
            .map_err(|value| WrapperError::NonUtf8EnvironmentValue {
                name: name.clone(),
                value,
            })?;
        environment.insert(name, value);
    }
    Ok(environment)
}

const WRAPPER_CONTROL_ENVIRONMENT: [&str; 19] = [
    super::descriptor_objects::LINKER_PROGRAM_ENV,
    super::descriptor_objects::LINKER_CARRIER_ENV,
    WRAPPER_MODE_ENV,
    DESCRIPTOR_STAGING_DIR_ENV,
    DESCRIPTOR_TARGET_DIR_ENV,
    SOURCE_EXCLUSION_DIR_ENV,
    FINAL_TARGET_DIR_ENV,
    FINAL_UNIT_MAP_PATH_ENV,
    FINAL_UNIT_MAP_HASH_ENV,
    FINAL_UNIT_PLAN_DIRECTORY_ENV,
    CARGO_UNIT_ID_ENV,
    FINAL_CARGO_UNIT_STATUS_ENV,
    DESCRIPTOR_PHASE_ENV,
    FINAL_PHASE_ENV,
    FINAL_PLAN_PATH_ENV,
    FINAL_PLAN_ID_ENV,
    FINAL_PLAN_FILE_HASH_ENV,
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
];

pub(crate) fn wrapper_control_environment(name: &str) -> bool {
    WRAPPER_CONTROL_ENVIRONMENT.contains(&name)
        || name.starts_with(FINAL_UNIT_INPUT_ENV_PREFIX)
        || name.starts_with(FINAL_UNIT_PLAN_ENV_PREFIX)
}

pub(crate) fn scrub_wrapper_control_environment(command: &mut Command) {
    for name in WRAPPER_CONTROL_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, _) in env::vars_os() {
        if name.to_str().is_some_and(|name| {
            name.starts_with(FINAL_UNIT_INPUT_ENV_PREFIX)
                || name.starts_with(FINAL_UNIT_PLAN_ENV_PREFIX)
        }) {
            command.env_remove(name);
        }
    }
}

/// Cargo/rustc environment with stable compile-time meaning.
///
/// This is deliberately an allowlist. Cargo injects target-directory-specific
/// variables such as `CARGO_TARGET_TMPDIR` and `CARGO_BIN_EXE_*`, while the
/// surrounding shell contributes arbitrary host/process state. Neither may
/// make a reusable unit ID depend on which Cargo process happened to run it.
/// Arbitrarily named build-script `cargo::rustc-env` values are recovered from
/// Cargo's exact build-script output below and added independently.
fn cargo_semantic_rustc_environment(name: &str) -> bool {
    matches!(
        name,
        "HOST"
            | "TARGET"
            | "PROFILE"
            | "OPT_LEVEL"
            | "DEBUG"
            | "CARGO_CRATE_NAME"
            | "CARGO_BIN_NAME"
            | "CARGO_PRIMARY_PACKAGE"
            | "CARGO_MANIFEST_LINKS"
    ) || name.starts_with("CARGO_PKG_")
        || name.starts_with("CARGO_CFG_")
        || name.starts_with("CARGO_FEATURE_")
}

fn identity_rustc_environment(
    environment: &BTreeMap<String, String>,
) -> io::Result<BTreeMap<String, String>> {
    let mut identity = environment
        .iter()
        .filter(|(name, _)| cargo_semantic_rustc_environment(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();

    let Some(out_dir) = environment.get("OUT_DIR") else {
        return Ok(identity);
    };
    for name in build_script_rustc_env_names(Path::new(out_dir))? {
        let value = environment.get(&name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Cargo build-script output declared rustc-env `{name}`, but rustc did not receive it"
                ),
            )
        })?;
        identity.insert(name, value.clone());
    }
    Ok(identity)
}

fn build_script_rustc_env_names(out_dir: &Path) -> io::Result<BTreeSet<String>> {
    let parent = out_dir.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cargo OUT_DIR has no build-script output directory",
        )
    })?;
    let output_path = parent.join("output");
    let output = fs::read_to_string(&output_path)?;
    let mut names = BTreeSet::new();
    for line in output.lines() {
        let directive = line
            .strip_prefix("cargo::rustc-env=")
            .or_else(|| line.strip_prefix("cargo:rustc-env="));
        let Some(directive) = directive else {
            continue;
        };
        let (name, _) = directive.split_once('=').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Cargo build-script output `{}` contains malformed rustc-env directive `{line}`",
                    output_path.display()
                ),
            )
        })?;
        if name.is_empty() || name.bytes().any(|byte| byte == 0 || byte == b'=') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Cargo build-script output `{}` contains invalid rustc-env name `{name}`",
                    output_path.display()
                ),
            ));
        }
        names.insert(name.to_string());
    }
    Ok(names)
}

fn realized_environment_hash(environment: &BTreeMap<String, String>) -> [u8; 32] {
    hash_canonical(REALIZED_ENVIRONMENT_HASH_DOMAIN, |hash| {
        hash.map("environment", environment);
    })
}

/// Stable 32-byte descriptor Cargo-unit identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DescriptorCargoUnitId([u8; 32]);

impl DescriptorCargoUnitId {
    /// Return the raw identifier bytes supplied to transport producers.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert the realized wrapper identity into the compiler ABI's strong
    /// Cargo-unit ID type.
    pub const fn as_abi_id(&self) -> CargoUnitId {
        CargoUnitId::from_bytes(self.0)
    }

    /// Parse the exact lowercase-hex representation written to sidecars.
    pub fn from_hex(encoded: &str) -> Result<Self, WrapperError> {
        if encoded.len() != 64 {
            return Err(WrapperError::InvalidCargoUnitId {
                value: encoded.to_string(),
                reason: format!(
                    "expected 64 lowercase hexadecimal characters, got {}",
                    encoded.len()
                ),
            });
        }
        let mut bytes = [0_u8; 32];
        for (index, output) in bytes.iter_mut().enumerate() {
            let high_offset = index * 2;
            let low_offset = high_offset + 1;
            let high = decode_lower_hex(encoded.as_bytes()[high_offset]).ok_or_else(|| {
                WrapperError::InvalidCargoUnitId {
                    value: encoded.to_string(),
                    reason: format!("invalid byte at offset {high_offset}"),
                }
            })?;
            let low = decode_lower_hex(encoded.as_bytes()[low_offset]).ok_or_else(|| {
                WrapperError::InvalidCargoUnitId {
                    value: encoded.to_string(),
                    reason: format!("invalid byte at offset {low_offset}"),
                }
            })?;
            *output = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

fn decode_lower_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

impl fmt::Debug for DescriptorCargoUnitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "DescriptorCargoUnitId({self})")
    }
}

impl fmt::Display for DescriptorCargoUnitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex_digest(&self.0))
    }
}

/// Path-free semantic inputs plus host-local join coordinates for one
/// descriptor-phase rustc unit.
///
/// Dependency paths never enter [`DescriptorCargoUnitId`]; only their exact
/// content digests do. The paths are retained solely so the root assembler can
/// join each rustc input to Cargo's same-invocation artifact enumeration
/// without scanning or hashing unrelated outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedRustcInvocation {
    crate_name: String,
    crate_types: BTreeSet<String>,
    target: Option<String>,
    edition: Option<String>,
    cfgs: BTreeSet<String>,
    features: BTreeSet<String>,
    emits: BTreeSet<String>,
    semantic_options: BTreeMap<String, String>,
    codegen_options: BTreeMap<String, String>,
    unstable_options: BTreeMap<String, String>,
    native_libraries: BTreeSet<String>,
    /// Exact rustc-visible values. These remain process-local and contribute
    /// only through the unit digest; sidecars never persist the plaintext.
    environment: BTreeMap<String, String>,
    rustc_identity_hash: [u8; 32],
    source_hash: [u8; 32],
    package_source_closure_hash: [u8; 32],
    dependencies: BTreeMap<String, [u8; 32]>,
    dependency_paths: BTreeMap<String, PathBuf>,
}

impl NormalizedRustcInvocation {
    fn cargo_extra_filename(&self) -> Result<Option<String>, String> {
        self.codegen_options
            .get("extra-filename")
            .map(|value| {
                validate_cargo_extra_filename(value)?;
                Ok(value.clone())
            })
            .transpose()
    }

    /// Derive the stable Cargo-unit ID for this descriptor compilation.
    pub fn cargo_unit_id(&self) -> DescriptorCargoUnitId {
        DescriptorCargoUnitId(hash_canonical(UNIT_HASH_DOMAIN, |hash| {
            hash.field("crate_name", self.crate_name.as_bytes());
            hash.set("crate_types", self.crate_types.iter().map(String::as_bytes));
            hash.optional("target", self.target.as_deref().map(str::as_bytes));
            hash.optional("edition", self.edition.as_deref().map(str::as_bytes));
            hash.set("cfgs", self.cfgs.iter().map(String::as_bytes));
            hash.set("features", self.features.iter().map(String::as_bytes));
            hash.set("emits", self.emits.iter().map(String::as_bytes));
            hash.map("semantic_options", &self.semantic_options);
            hash.map("codegen_options", &self.codegen_options);
            hash.map("unstable_options", &self.unstable_options);
            hash.set(
                "native_libraries",
                self.native_libraries.iter().map(String::as_bytes),
            );
            hash.map("environment", &self.environment);
            hash.field("rustc_identity_hash", &self.rustc_identity_hash);
            hash.field("source_hash", &self.source_hash);
            hash.field(
                "package_source_closure_hash",
                &self.package_source_closure_hash,
            );
            hash.digest_map("dependencies", &self.dependencies);
            hash.set(
                "descriptor_codegen",
                DESCRIPTOR_CODEGEN_POLICY
                    .iter()
                    .map(|value| value.as_bytes()),
            );
            hash.field("descriptor_strip", DESCRIPTOR_STRIP_POLICY.as_bytes());
        }))
    }

    fn normalized_inputs_hash(&self) -> [u8; 32] {
        self.cargo_unit_id().0
    }

    fn logical_environment_hash(&self) -> [u8; 32] {
        hash_canonical(LOGICAL_ENVIRONMENT_HASH_DOMAIN, |hash| {
            hash.map("environment", &self.environment);
        })
    }

    fn final_match_profile_hash(&self) -> [u8; 32] {
        let profile_codegen_options = self
            .codegen_options
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "metadata" | "extra-filename"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let profile_environment = self
            .environment
            .iter()
            .filter(|(name, _)| matches!(name.as_str(), "PROFILE" | "DEBUG" | "OPT_LEVEL"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        hash_canonical(PROFILE_HASH_DOMAIN, |hash| {
            hash.map("codegen_options", &profile_codegen_options);
            hash.map("unstable_options", &self.unstable_options);
            hash.map("semantic_options", &self.semantic_options);
            hash.map("cargo_profile_environment", &profile_environment);
        })
    }
}

/// Normalize one rustc invocation without admitting host-local source,
/// dependency, output, sysroot, or linker paths into semantic identity.
///
/// Exact dependency paths are retained as non-semantic Cargo join coordinates.
pub fn normalize_rustc_invocation(
    args: &[OsString],
    environment: &BTreeMap<String, String>,
    rustc_identity: &[u8],
) -> Result<NormalizedRustcInvocation, WrapperError> {
    normalize_rustc_invocation_with_package_root(args, environment, rustc_identity, None, None)
}

/// Normalize one rustc invocation and bind it to the complete regular-file
/// closure beneath an explicit Cargo package root.
///
/// Cargo's wrappers call this form with `CARGO_MANIFEST_DIR`. The public
/// path-only normalizer above retains a crate-root-only fallback for direct
/// rustc fixtures and non-Cargo callers that have no package provenance.
fn normalize_rustc_invocation_with_package_root(
    args: &[OsString],
    environment: &BTreeMap<String, String>,
    rustc_identity: &[u8],
    package_root: Option<&Path>,
    excluded_source_subtree: Option<&Path>,
) -> Result<NormalizedRustcInvocation, WrapperError> {
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .map(str::to_owned)
                .ok_or_else(|| WrapperError::NonUtf8RustcArgument(arg.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut crate_name = None;
    let mut crate_types = BTreeSet::new();
    let mut target = None;
    let mut edition = None;
    let mut cfgs = BTreeSet::new();
    let mut emits = BTreeSet::new();
    let mut semantic_options = BTreeMap::new();
    let mut codegen_options = BTreeMap::new();
    let mut unstable_options = BTreeMap::new();
    let mut native_libraries = BTreeSet::new();
    let mut dependencies = BTreeMap::new();
    let mut dependency_paths = BTreeMap::new();
    let mut source_hash = None;
    let mut index = 0;

    while index < args.len() {
        let argument = &args[index];

        if let Some(value) = long_option_value(&args, &mut index, "--crate-name")? {
            crate_name = Some(value);
        } else if let Some(value) = long_option_value(&args, &mut index, "--crate-type")? {
            crate_types.extend(split_comma_values(&value));
        } else if let Some(value) = long_option_value(&args, &mut index, "--target")? {
            target = Some(value);
        } else if let Some(value) = long_option_value(&args, &mut index, "--edition")? {
            edition = Some(value);
        } else if let Some(value) = long_option_value(&args, &mut index, "--cfg")? {
            cfgs.insert(value);
        } else if let Some(value) = long_option_value(&args, &mut index, "--emit")? {
            for emit in split_comma_values(&value) {
                let kind = emit.split_once('=').map_or(emit.as_str(), |(kind, _)| kind);
                emits.insert(kind.to_string());
            }
        } else if let Some(value) = long_option_value(&args, &mut index, "--extern")? {
            let (name, path) = parse_extern(&value)?;
            match path {
                Some(path) => {
                    if !path.is_absolute() {
                        return Err(WrapperError::RelativeExternArtifact(path));
                    }
                    let digest = hash_file(&path, FILE_HASH_DOMAIN).map_err(|source| {
                        WrapperError::HashInput {
                            path: path.clone(),
                            source,
                        }
                    })?;
                    if let Some(previous) = dependencies.insert(name.clone(), digest) {
                        if previous != digest {
                            return Err(WrapperError::ConflictingExtern(name));
                        }
                    }
                    if let Some(previous) = dependency_paths.insert(name.clone(), path.clone()) {
                        if previous != path {
                            return Err(WrapperError::ConflictingExtern(name));
                        }
                    }
                }
                None => {
                    // `--extern proc_macro` and similar pathless externs are
                    // rustc/sysroot capabilities, not Cargo artifact edges.
                    // Keep them in unit identity without inventing an artifact
                    // hash that the root linker could never resolve.
                    semantic_options.insert(format!("rustc-resolved-extern:{name}"), String::new());
                }
            }
        } else if let Some(value) = short_option_value(&args, &mut index, "-C")? {
            let (key, value) = split_key_value(&value);
            if !descriptor_codegen_option(key) && !linker_only_codegen_option(key) {
                codegen_options.insert(key.to_string(), value.to_string());
            }
        } else if let Some(value) = short_option_value(&args, &mut index, "-Z")? {
            let (key, value) = split_key_value(&value);
            if key == "binary-dep-depinfo" {
                return Err(WrapperError::UnsupportedBinaryDepDepInfo);
            }
            if matches!(key, "remap-cwd-prefix" | "remap-path-scope") {
                return Err(WrapperError::UnsupportedDepInfoPathRemapping(format!(
                    "-Z{key}"
                )));
            }
            if !path_only_unstable_option(key) {
                unstable_options.insert(key.to_string(), value.to_string());
            }
        } else if let Some(value) = short_option_value(&args, &mut index, "-l")? {
            native_libraries.insert(value);
        } else if let Some(value) = short_option_value(&args, &mut index, "-L")? {
            let kind = value
                .split_once('=')
                .map_or("all", |(kind, _)| kind)
                .to_string();
            semantic_options.insert(format!("library-search-kind:{kind}"), String::new());
        } else if long_option_value(&args, &mut index, "--env-set")?.is_some() {
            return Err(WrapperError::UnsupportedLogicalEnvironmentInjection);
        } else if long_option_value(&args, &mut index, "--remap-path-prefix")?.is_some() {
            return Err(WrapperError::UnsupportedDepInfoPathRemapping(
                "--remap-path-prefix".to_string(),
            ));
        } else if long_option_value(&args, &mut index, "--remap-path-scope")?.is_some() {
            return Err(WrapperError::UnsupportedDepInfoPathRemapping(
                "--remap-path-scope".to_string(),
            ));
        } else if consume_path_option(&args, &mut index, argument)? {
            // Output, sysroot, and diagnostics paths do not participate in a
            // descriptor unit's host-independent semantic identity.
        } else if consume_diagnostic_option(&args, &mut index, argument)? {
            // Diagnostics do not affect emitted transport records.
        } else if boolean_rustc_flag(argument) {
            semantic_options.insert(argument.clone(), String::new());
        } else if argument.starts_with('-') {
            if let Some((key, value)) = argument.split_once('=') {
                semantic_options.insert(key.to_string(), value.to_string());
            } else {
                return Err(WrapperError::UnsupportedRustcOption(argument.clone()));
            }
        } else {
            if source_hash.is_some() {
                return Err(WrapperError::MultipleRustcInputs);
            }
            let path = PathBuf::from(argument);
            source_hash = Some(
                hash_file(&path, FILE_HASH_DOMAIN)
                    .map_err(|source| WrapperError::HashInput { path, source })?,
            );
        }

        index += 1;
    }

    let crate_name = crate_name.ok_or(WrapperError::MissingCrateName)?;
    if crate_types.is_empty() {
        // rustc's `--test` mode implies a binary test harness and Cargo
        // therefore omits an explicit `--crate-type`. Preserve the exact
        // effective crate type while continuing to reject an unexplained
        // missing type for every other invocation.
        if semantic_options.contains_key("--test") {
            crate_types.insert("bin".to_string());
        } else {
            return Err(WrapperError::MissingCrateType);
        }
    }
    let source_hash = source_hash.ok_or(WrapperError::MissingRustcInput)?;
    let package_source_closure_hash = match package_root {
        Some(package_root) => hash_package_source_closure(package_root, excluded_source_subtree)
            .map_err(|source| WrapperError::HashPackageSourceClosure {
                path: package_root.to_path_buf(),
                source,
            })?,
        None => hash_canonical(PACKAGE_SOURCE_CLOSURE_HASH_DOMAIN, |hash| {
            hash.field("mode", b"crate-root-only-fallback");
            hash.field("crate_root", &source_hash);
        }),
    };
    let features = cfgs
        .iter()
        .filter_map(|cfg| {
            cfg.strip_prefix("feature=\"")
                .and_then(|feature| feature.strip_suffix('"'))
                .map(str::to_owned)
        })
        .collect();
    let identity_environment = identity_rustc_environment(environment).map_err(|source| {
        WrapperError::ReadBuildScriptEnvironment {
            out_dir: environment.get("OUT_DIR").map(PathBuf::from),
            source,
        }
    })?;

    Ok(NormalizedRustcInvocation {
        crate_name,
        crate_types,
        target,
        edition,
        cfgs,
        features,
        emits,
        semantic_options,
        codegen_options,
        unstable_options,
        native_libraries,
        environment: identity_environment,
        rustc_identity_hash: hash_bytes(FILE_HASH_DOMAIN, rustc_identity),
        source_hash,
        package_source_closure_hash,
        dependencies,
        dependency_paths,
    })
}

fn long_option_value(
    args: &[String],
    index: &mut usize,
    option: &'static str,
) -> Result<Option<String>, WrapperError> {
    let argument = &args[*index];
    if argument == option {
        *index += 1;
        return args
            .get(*index)
            .cloned()
            .map(Some)
            .ok_or(WrapperError::MissingRustcOptionValue(option));
    }
    Ok(argument
        .strip_prefix(option)
        .and_then(|suffix| suffix.strip_prefix('='))
        .map(str::to_owned))
}

fn short_option_value(
    args: &[String],
    index: &mut usize,
    option: &'static str,
) -> Result<Option<String>, WrapperError> {
    let argument = &args[*index];
    if argument == option {
        *index += 1;
        return args
            .get(*index)
            .cloned()
            .map(Some)
            .ok_or(WrapperError::MissingRustcOptionValue(option));
    }
    Ok(argument
        .strip_prefix(option)
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_owned))
}

fn consume_path_option(
    args: &[String],
    index: &mut usize,
    argument: &str,
) -> Result<bool, WrapperError> {
    for option in ["--out-dir", "--sysroot", "-o"] {
        if argument == option {
            *index += 1;
            if args.get(*index).is_none() {
                return Err(WrapperError::MissingRustcOptionValue(option));
            }
            return Ok(true);
        }
        if argument.starts_with(&format!("{option}=")) {
            return Ok(true);
        }
        if option == "-o" && argument.starts_with("-o") && argument.len() > 2 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn consume_diagnostic_option(
    args: &[String],
    index: &mut usize,
    argument: &str,
) -> Result<bool, WrapperError> {
    const OPTIONS: [&str; 12] = [
        "--allow",
        "--cap-lints",
        "--check-cfg",
        "--color",
        "--deny",
        "--diagnostic-width",
        "--error-format",
        "--forbid",
        "--force-warn",
        "--json",
        "--warn",
        "--crate-version",
    ];
    for option in OPTIONS {
        if argument == option {
            *index += 1;
            if args.get(*index).is_none() {
                return Err(WrapperError::MissingRustcOptionValue(option));
            }
            return Ok(true);
        }
        if argument.starts_with(&format!("{option}=")) {
            return Ok(true);
        }
    }
    for option in ["-A", "-D", "-F", "-W"] {
        if argument == option {
            *index += 1;
            if args.get(*index).is_none() {
                return Err(WrapperError::MissingRustcOptionValue("lint option"));
            }
            return Ok(true);
        }
        if argument.starts_with(option) && argument.len() > option.len() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn boolean_rustc_flag(argument: &str) -> bool {
    matches!(
        argument,
        "--test" | "--bench" | "--no-main" | "--proc-macro" | "--crate-type" | "--verbose" | "-v"
    )
}

fn split_comma_values(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_extern(value: &str) -> Result<(String, Option<PathBuf>), WrapperError> {
    let (name, path) = value
        .split_once('=')
        .map_or((value, None), |(name, path)| (name, Some(path)));
    if name.is_empty() {
        return Err(WrapperError::MalformedExtern(value.to_string()));
    }
    Ok((name.to_string(), path.map(PathBuf::from)))
}

fn split_key_value(value: &str) -> (&str, &str) {
    value
        .split_once('=')
        .map_or((value, ""), |(key, value)| (key, value))
}

fn descriptor_codegen_option(key: &str) -> bool {
    matches!(
        key,
        "codegen-units" | "incremental" | "link-dead-code" | "linker-plugin-lto" | "lto" | "strip"
    )
}

fn linker_only_codegen_option(key: &str) -> bool {
    matches!(
        key,
        "link-arg"
            | "link-args"
            | "linker"
            | "linker-features"
            | "rpath"
            | "default-linker-libraries"
    )
}

fn path_only_unstable_option(key: &str) -> bool {
    matches!(
        key,
        "incremental-info"
            | "profile"
            | "profile-emit"
            | "profile-use"
            | "self-profile"
            | "temps-dir"
    )
}

#[derive(Debug)]
enum DescriptorCarrierMode {
    Rlib(PathBuf),
    Native(OsString),
    Single,
}

fn rustc_codegen_value(args: &[OsString], key: &str) -> Result<Option<String>, WrapperError> {
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .map(str::to_owned)
                .ok_or_else(|| WrapperError::NonUtf8RustcArgument(arg.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut value = None;
    let mut index = 0;
    while index < args.len() {
        if let Some(option) = short_option_value(&args, &mut index, "-C")? {
            let (name, option_value) = split_key_value(&option);
            if name == key {
                value = Some(option_value.to_owned());
            }
        }
        index += 1;
    }
    Ok(value)
}

fn descriptor_codegen_units(args: &[OsString]) -> Result<u32, WrapperError> {
    match rustc_codegen_value(args, "codegen-units")? {
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                WrapperError::DescriptorCarrier(format!("invalid codegen-units: {value}"))
            }),
        None => Ok(if rustc_codegen_value(args, "incremental")?.is_some() {
            // Descriptor-only artifacts benefit from fewer cached object files.
            // Preserve an explicit Cargo setting and final-app partitioning.
            DESCRIPTOR_INCREMENTAL_CODEGEN_UNITS
        } else {
            16
        }),
    }
}

fn descriptor_carrier_mode(
    input: &NormalizedRustcInvocation,
    dep_info: &Path,
    args: &[OsString],
) -> Result<DescriptorCarrierMode, WrapperError> {
    // Explicit output routing is supported by the single-object path. Do
    // not infer an rlib filename when rustc was given an exact different one.
    if descriptor_has_explicit_output(args)? || !input.emits.contains("link") {
        return Ok(DescriptorCarrierMode::Single);
    }
    if input.crate_types.len() == 1
        && input
            .crate_types
            .iter()
            .all(|kind| matches!(kind.as_str(), "lib" | "rlib"))
    {
        let extra = input
            .cargo_extra_filename()
            .map_err(WrapperError::DescriptorCarrier)?;
        return Ok(DescriptorCarrierMode::Rlib(dep_info.with_file_name(
            format!(
                "lib{}{}.rlib",
                input.crate_name,
                extra.as_deref().unwrap_or_default()
            ),
        )));
    }
    // The native adapter currently supports host macOS/Linux cc drivers with
    // explicit object arguments. Unknown targets/linker protocols retain the
    // complete single-object carrier while still allowing incremental queries.
    if cfg!(any(target_os = "macos", target_os = "linux"))
        && input.target.is_none()
        && rustc_codegen_value(args, "linker-flavor")?.is_none()
        && input.crate_types.len() == 1
        && input
            .crate_types
            .iter()
            .all(|kind| matches!(kind.as_str(), "bin" | "proc-macro" | "dylib" | "cdylib"))
    {
        let linker = rustc_codegen_value(args, "linker")?.unwrap_or_else(|| "cc".to_owned());
        let name = Path::new(&linker).file_name().and_then(OsStr::to_str);
        if matches!(name, Some("cc" | "gcc" | "clang")) {
            return Ok(DescriptorCarrierMode::Native(linker.into()));
        }
    }
    Ok(DescriptorCarrierMode::Single)
}

fn descriptor_has_explicit_output(args: &[OsString]) -> Result<bool, WrapperError> {
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .map(str::to_owned)
                .ok_or_else(|| WrapperError::NonUtf8RustcArgument(arg.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut index = 0;
    while index < args.len() {
        if short_option_value(&args, &mut index, "-o")?.is_some() {
            return Ok(true);
        }
        if let Some(value) = long_option_value(&args, &mut index, "--emit")? {
            if value.split(',').any(|emit| emit.starts_with("link=")) {
                return Ok(true);
            }
        }
        index += 1;
    }
    Ok(false)
}

#[cfg(test)]
fn descriptor_rustc_args(args: &[OsString], object: &Path) -> Result<Vec<OsString>, WrapperError> {
    descriptor_rustc_args_with_carrier(args, object, &DescriptorCarrierMode::Single)
}

fn descriptor_rustc_args_with_carrier(
    rustc_args: &[OsString],
    descriptor_object: &Path,
    carrier: &DescriptorCarrierMode,
) -> Result<Vec<OsString>, WrapperError> {
    let mut result = Vec::with_capacity(rustc_args.len() + 6);
    let mut index = 0;
    while index < rustc_args.len() {
        let argument = rustc_args[index]
            .to_str()
            .ok_or_else(|| WrapperError::NonUtf8RustcArgument(rustc_args[index].clone()))?;

        if argument == "-C" {
            let value = rustc_args
                .get(index + 1)
                .and_then(|value| value.to_str())
                .ok_or(WrapperError::MissingRustcOptionValue("-C"))?;
            if descriptor_override_option(split_key_value(value).0, carrier) {
                index += 2;
                continue;
            }
        } else if let Some(value) = argument.strip_prefix("-C") {
            if !value.is_empty() && descriptor_override_option(split_key_value(value).0, carrier) {
                index += 1;
                continue;
            }
        }

        if argument == "--emit" {
            let value = rustc_args
                .get(index + 1)
                .and_then(|value| value.to_str())
                .ok_or(WrapperError::MissingRustcOptionValue("--emit"))?;
            if let Some(rewritten) = emit_without_object(value) {
                result.push(OsString::from("--emit"));
                result.push(OsString::from(rewritten));
            }
            index += 2;
            continue;
        } else if let Some(value) = argument.strip_prefix("--emit=") {
            if let Some(rewritten) = emit_without_object(value) {
                result.push(OsString::from(format!("--emit={rewritten}")));
            }
            index += 1;
            continue;
        }

        result.push(rustc_args[index].clone());
        index += 1;
    }

    result.extend([
        OsString::from("-Clink-dead-code=yes"),
        OsString::from("-Clto=off"),
        OsString::from("-Cstrip=none"),
        // Descriptor extraction needs symbols and relocations, not DWARF.
        // Keep the original Cargo debug setting in normalized final-profile
        // identity; only this intermediate rustc invocation overrides it.
        OsString::from("-Cdebuginfo=0"),
    ]);
    match carrier {
        DescriptorCarrierMode::Single => result.extend([
            OsString::from("-Ccodegen-units=1"),
            OsString::from(format!("--emit=obj={}", descriptor_object.display())),
        ]),
        DescriptorCarrierMode::Native(_) => {
            let executable = env::current_exe().map_err(WrapperError::CurrentDirectory)?;
            result.push(OsString::from(format!("-Clinker={}", executable.display())));
        }
        DescriptorCarrierMode::Rlib(_) => {}
    }
    if !matches!(carrier, DescriptorCarrierMode::Single)
        && rustc_codegen_value(rustc_args, "codegen-units")?.is_none()
        && rustc_codegen_value(rustc_args, "incremental")?.is_some()
    {
        result.push(OsString::from(format!(
            "-Ccodegen-units={DESCRIPTOR_INCREMENTAL_CODEGEN_UNITS}"
        )));
    }
    Ok(result)
}

fn descriptor_override_option(key: &str, carrier: &DescriptorCarrierMode) -> bool {
    matches!(
        key,
        "link-dead-code" | "linker-plugin-lto" | "lto" | "strip" | "debuginfo"
    ) || (key == "codegen-units" && matches!(carrier, DescriptorCarrierMode::Single))
}

fn emit_without_object(value: &str) -> Option<String> {
    let emits = value
        .split(',')
        .filter(|emit| emit.split_once('=').map_or(*emit, |(kind, _)| kind) != "obj")
        .collect::<Vec<_>>();
    (!emits.is_empty()).then(|| emits.join(","))
}

/// Hash the conservative source closure rooted at one Cargo package.
///
/// Every regular file participates under its UTF-8, slash-separated path
/// relative to `package_root`, so relocating an identical package does not
/// change the digest. Exact-name VCS entries (`.git`, `.hg`, `.svn`,
/// `.jj`), `node_modules` at any depth, the conventional package-root
/// `target` directory, and one caller-proven exact generated subtree are
/// excluded. The latter is the runner-owned descriptor target. Nested source
/// directories named `target` remain ordinary inputs, and an untrusted
/// `.rustc_info.json` file never hides its containing subtree. All other files
/// participate, including manifests, build
/// scripts, Rust modules, and `include_*` assets; this deliberately accepts
/// false invalidations in exchange for never guessing which package-local
/// files rustc may observe.
///
/// Symlinks, special files, and non-UTF-8 relative paths fail closed because
/// following or encoding them would make the identity host-dependent. Files
/// outside `CARGO_MANIFEST_DIR` are not discoverable from rustc's
/// pre-compilation argv. The wrapper separately audits rustc-declared
/// `OUT_DIR` source dependencies from the successful compile's exact dep-info;
/// other external source classes require an explicit tracking protocol.
fn hash_package_source_closure(
    package_root: &Path,
    excluded_subtree: Option<&Path>,
) -> io::Result<[u8; 32]> {
    let package_root = fs::canonicalize(package_root)?;
    let metadata = fs::symlink_metadata(&package_root)?;
    if !metadata.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cargo package source root is not a directory",
        ));
    }

    let excluded_subtree = excluded_subtree
        .map(fs::canonicalize)
        .transpose()?
        .filter(|excluded| excluded.starts_with(&package_root));
    let mut files = BTreeMap::new();
    collect_package_source_files(
        &package_root,
        &package_root,
        excluded_subtree.as_deref(),
        &mut files,
    )?;
    Ok(hash_canonical(PACKAGE_SOURCE_CLOSURE_HASH_DOMAIN, |hash| {
        hash.field("mode", b"manifest-tree-v4-root-target");
        hash.digest_map("files", &files);
    }))
}

fn collect_package_source_files(
    package_root: &Path,
    directory: &Path,
    excluded_subtree: Option<&Path>,
    files: &mut BTreeMap<String, [u8; 32]>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if package_source_root_vcs_entry_is_excluded(package_root, directory, &name) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if excluded_subtree.is_some_and(|excluded| entry.path() == excluded) {
                continue;
            }
            if package_source_generated_directory_is_excluded(package_root, directory, &name) {
                continue;
            }
            collect_package_source_files(package_root, &entry.path(), excluded_subtree, files)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "package source closure contains unsupported symlink or special file `{}`",
                    entry.path().display()
                ),
            ));
        }

        let path = entry.path();
        let relative = path.strip_prefix(package_root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "package source file `{}` escaped root `{}`",
                    path.display(),
                    package_root.display()
                ),
            )
        })?;
        let relative = portable_package_relative_path(relative)?;
        if relative
            .rsplit('/')
            .next()
            .is_some_and(|name| name.ends_with(' '))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("package source path is ambiguous in rustc dep-info: `{relative}`"),
            ));
        }
        let digest = hash_file(&path, PACKAGE_SOURCE_FILE_HASH_DOMAIN)?;
        if files.insert(relative.clone(), digest).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("package source closure repeats relative path `{relative}`"),
            ));
        }
    }
    Ok(())
}

fn package_source_root_vcs_entry_is_excluded(
    package_root: &Path,
    parent: &Path,
    name: &OsStr,
) -> bool {
    parent == package_root && matches!(name.to_str(), Some(".git" | ".hg" | ".svn" | ".jj"))
}

fn package_source_generated_directory_is_excluded(
    package_root: &Path,
    parent: &Path,
    name: &OsStr,
) -> bool {
    matches!(name.to_str(), Some("node_modules"))
        || (parent == package_root && name == OsStr::new("target"))
}

fn rustc_dep_info_path(
    rustc_args: &[OsString],
    input: &NormalizedRustcInvocation,
) -> Result<PathBuf, WrapperError> {
    let args = rustc_args
        .iter()
        .map(|argument| {
            argument
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| WrapperError::NonUtf8RustcArgument(argument.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut dep_info = None;
    let mut out_dir = None;
    let mut explicit_output = false;
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if let Some(value) = long_option_value(&args, &mut index, "--emit")? {
            for emit in value.split(',').filter(|emit| !emit.is_empty()) {
                let (kind, path) = emit
                    .split_once('=')
                    .map_or((emit, None), |(kind, path)| (kind, Some(path)));
                if kind == "dep-info" {
                    if dep_info.is_some() {
                        return Err(WrapperError::DuplicateDepInfoOutput);
                    }
                    dep_info = Some(path.map(PathBuf::from));
                }
            }
        } else if let Some(value) = long_option_value(&args, &mut index, "--out-dir")? {
            out_dir = Some(PathBuf::from(value));
        } else if argument == "-o"
            || argument.starts_with("-o=")
            || (argument.starts_with("-o") && argument.len() > 2)
        {
            explicit_output = true;
            if argument == "-o" {
                index += 1;
                if args.get(index).is_none() {
                    return Err(WrapperError::MissingRustcOptionValue("-o"));
                }
            }
        }
        index += 1;
    }

    let explicit = dep_info.ok_or(WrapperError::MissingDepInfoOutput)?;
    let path = match explicit {
        Some(path) => path,
        None if explicit_output => return Err(WrapperError::UnsupportedDefaultDepInfoWithOutput),
        None => {
            let out_dir = out_dir.ok_or(WrapperError::MissingRustcOptionValue("--out-dir"))?;
            let extra = input.cargo_extra_filename().map_err(|reason| {
                WrapperError::InvalidCargoExtraFilename {
                    context: "rustc dep-info output",
                    value: input
                        .codegen_options
                        .get("extra-filename")
                        .cloned()
                        .unwrap_or_default(),
                    reason,
                }
            })?;
            out_dir.join(format!(
                "{}{}.d",
                input.crate_name,
                extra.as_deref().unwrap_or_default()
            ))
        }
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(WrapperError::CurrentDirectory)
    }
}

struct ParsedDepInfo {
    source_paths: Vec<PathBuf>,
    environment: Vec<ParsedEnvironmentDependency>,
}

struct ParsedEnvironmentDependency {
    name: String,
    value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedRustcInputs {
    out_dir_dependencies: Vec<DescriptorOutDirDependency>,
    source_dependencies: Vec<DescriptorSourceDependency>,
    environment_dependencies: Vec<DescriptorEnvironmentDependency>,
}

fn observe_rustc_inputs(
    dep_info_path: &Path,
    working_directory: &Path,
    out_dir: Option<&Path>,
    out_dir_coordinate: Option<&str>,
    phase_environment: &BTreeMap<String, Option<String>>,
) -> Result<ObservedRustcInputs, WrapperError> {
    let metadata =
        fs::symlink_metadata(dep_info_path).map_err(|source| WrapperError::ReadDepInfo {
            path: dep_info_path.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(WrapperError::UnsafeDepInfoOutput(
            dep_info_path.to_path_buf(),
        ));
    }
    let bytes = fs::read(dep_info_path).map_err(|source| WrapperError::ReadDepInfo {
        path: dep_info_path.to_path_buf(),
        source,
    })?;
    let parsed = parse_rustc_dep_info(&bytes).map_err(|reason| WrapperError::MalformedDepInfo {
        path: dep_info_path.to_path_buf(),
        reason,
    })?;

    let working_directory = canonical_plain_directory(working_directory)?;
    let original_out_dir = out_dir;
    let canonical_out_dir = out_dir.map(canonical_plain_out_dir).transpose()?;
    match (canonical_out_dir.as_ref(), out_dir_coordinate) {
        (Some(_), Some(coordinate)) => validate_portable_relative_path(coordinate)
            .map_err(WrapperError::InvalidObservedOutDirCoordinate)?,
        (None, None) => {}
        _ => return Err(WrapperError::InconsistentObservedOutDir),
    }

    let mut out_dir_dependencies = BTreeMap::<String, String>::new();
    let mut source_dependencies = BTreeMap::<PathBuf, String>::new();
    for source_path in parsed.source_paths {
        let source_path = if source_path.is_absolute() {
            source_path
        } else {
            working_directory.join(source_path)
        };
        let lexical_out_dir_relative =
            original_out_dir.and_then(|out_dir| source_path.strip_prefix(out_dir).ok());
        if let Some(relative) = lexical_out_dir_relative {
            portable_package_relative_path(relative).map_err(|source| {
                WrapperError::ReadOutDirDependency {
                    path: source_path.clone(),
                    source,
                }
            })?;
            validate_no_symlink_path(
                original_out_dir.expect("a lexical OUT_DIR relative path has an OUT_DIR"),
                relative,
            )?;
        }

        let original_metadata = fs::symlink_metadata(&source_path).map_err(|source| {
            WrapperError::ReadObservedSource {
                path: source_path.clone(),
                source,
            }
        })?;
        if original_metadata.file_type().is_symlink() || !original_metadata.file_type().is_file() {
            return Err(WrapperError::UnsafeObservedSource(source_path));
        }
        let canonical =
            fs::canonicalize(&source_path).map_err(|source| WrapperError::ReadObservedSource {
                path: source_path.clone(),
                source,
            })?;
        let canonical_metadata = fs::symlink_metadata(&canonical).map_err(|source| {
            WrapperError::ReadObservedSource {
                path: canonical.clone(),
                source,
            }
        })?;
        if canonical_metadata.file_type().is_symlink() || !canonical_metadata.file_type().is_file()
        {
            return Err(WrapperError::UnsafeObservedSource(canonical));
        }

        if let Some(canonical_out_dir) = canonical_out_dir.as_ref() {
            match canonical.strip_prefix(canonical_out_dir) {
                Ok(relative) if relative.as_os_str().is_empty() => {
                    return Err(WrapperError::UnsafeOutDirDependency(canonical));
                }
                Ok(relative) => {
                    if lexical_out_dir_relative.is_none() {
                        // An external lexical path that reaches OUT_DIR only
                        // through a symlink is not a portable generated-input
                        // coordinate.
                        return Err(WrapperError::UnsafeOutDirDependency(source_path));
                    }
                    validate_no_symlink_path(canonical_out_dir, relative)?;
                    let relative_path =
                        portable_package_relative_path(relative).map_err(|source| {
                            WrapperError::ReadOutDirDependency {
                                path: canonical.clone(),
                                source,
                            }
                        })?;
                    let content_hash =
                        hash_file(&canonical, OUT_DIR_FILE_HASH_DOMAIN).map_err(|source| {
                            WrapperError::ReadOutDirDependency {
                                path: canonical,
                                source,
                            }
                        })?;
                    if out_dir_dependencies
                        .insert(relative_path.clone(), hex_digest(&content_hash))
                        .is_some()
                    {
                        return Err(WrapperError::ConflictingOutDirDependency(relative_path));
                    }
                    continue;
                }
                Err(_) if lexical_out_dir_relative.is_some() => {
                    return Err(WrapperError::UnsafeOutDirDependency(source_path));
                }
                Err(_) => {}
            }
        }

        let content_hash =
            hash_file(&canonical, OBSERVED_SOURCE_FILE_HASH_DOMAIN).map_err(|source| {
                WrapperError::ReadObservedSource {
                    path: canonical.clone(),
                    source,
                }
            })?;
        record_observed_source_dependency(
            &mut source_dependencies,
            canonical,
            hex_digest(&content_hash),
        )?;
    }

    let environment_dependencies = normalize_environment_dependencies(
        parsed.environment,
        original_out_dir,
        out_dir_coordinate,
        phase_environment,
    )?;

    Ok(ObservedRustcInputs {
        out_dir_dependencies: out_dir_dependencies
            .into_iter()
            .map(|(relative_path, content_hash)| DescriptorOutDirDependency {
                relative_path,
                content_hash,
            })
            .collect(),
        source_dependencies: source_dependencies
            .into_iter()
            .map(|(absolute_path, content_hash)| DescriptorSourceDependency {
                absolute_path,
                content_hash,
            })
            .collect(),
        environment_dependencies,
    })
}

fn record_observed_source_dependency(
    source_dependencies: &mut BTreeMap<PathBuf, String>,
    canonical: PathBuf,
    content_hash: String,
) -> Result<(), WrapperError> {
    match source_dependencies.entry(canonical.clone()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(content_hash);
        }
        std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &content_hash => {
            // rustc can name the same included source through multiple lexical
            // paths (for example `docs/file.md` and
            // `handler/../docs/file.md`). The canonical input and bytes are
            // identical, so this is one dependency record.
        }
        std::collections::btree_map::Entry::Occupied(_) => {
            return Err(WrapperError::ConflictingObservedSource(canonical));
        }
    }
    Ok(())
}

fn canonical_plain_directory(path: &Path) -> Result<PathBuf, WrapperError> {
    if !path.is_absolute() {
        return Err(WrapperError::RelativeDepInfoWorkingDirectory(
            path.to_path_buf(),
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|source| WrapperError::ReadObservedSource {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata =
        fs::symlink_metadata(&canonical).map_err(|source| WrapperError::ReadObservedSource {
            path: canonical.clone(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(WrapperError::UnsafeObservedSource(canonical));
    }
    Ok(canonical)
}

fn canonical_plain_out_dir(path: &Path) -> Result<PathBuf, WrapperError> {
    if !path.is_absolute() {
        return Err(WrapperError::RelativeOutDir(path.to_path_buf()));
    }
    let canonical =
        fs::canonicalize(path).map_err(|source| WrapperError::ReadOutDirDependency {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata =
        fs::symlink_metadata(&canonical).map_err(|source| WrapperError::ReadOutDirDependency {
            path: canonical.clone(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(WrapperError::UnsafeOutDirDependency(canonical));
    }
    Ok(canonical)
}

fn normalize_environment_dependencies(
    dependencies: Vec<ParsedEnvironmentDependency>,
    out_dir: Option<&Path>,
    out_dir_coordinate: Option<&str>,
    phase_environment: &BTreeMap<String, Option<String>>,
) -> Result<Vec<DescriptorEnvironmentDependency>, WrapperError> {
    let mut normalized = BTreeMap::<String, Option<String>>::new();
    for dependency in dependencies {
        if wrapper_control_environment(&dependency.name) {
            let expected = phase_environment
                .get(&dependency.name)
                .cloned()
                .unwrap_or(None);
            if dependency.value != expected {
                return Err(WrapperError::PhaseOwnedEnvironmentMismatch {
                    name: dependency.name,
                });
            }
            continue;
        }

        let value_hash = match dependency.value {
            None => None,
            Some(value) if dependency.name == "OUT_DIR" => {
                let expected = out_dir.ok_or(WrapperError::InconsistentObservedOutDir)?;
                if Path::new(&value) != expected {
                    return Err(WrapperError::ObservedEnvironmentMismatch {
                        name: dependency.name,
                    });
                }
                let coordinate =
                    out_dir_coordinate.ok_or(WrapperError::InconsistentObservedOutDir)?;
                Some(observed_environment_value_hash(
                    &dependency.name,
                    b"out-dir-coordinate",
                    coordinate.as_bytes(),
                ))
            }
            Some(value) => Some(observed_environment_value_hash(
                &dependency.name,
                b"exact-value",
                value.as_bytes(),
            )),
        };
        if normalized
            .insert(dependency.name.clone(), value_hash)
            .is_some()
        {
            return Err(WrapperError::DuplicateEnvironmentDependency {
                name: dependency.name,
            });
        }
    }
    Ok(normalized
        .into_iter()
        .map(|(name, value_hash)| DescriptorEnvironmentDependency { name, value_hash })
        .collect())
}

fn observed_environment_value_hash(name: &str, mode: &[u8], value: &[u8]) -> String {
    hex_digest(&hash_canonical(
        OBSERVED_ENVIRONMENT_VALUE_HASH_DOMAIN,
        |hash| {
            hash.field("name", name.as_bytes());
            hash.field("mode", mode);
            hash.field("value", value);
        },
    ))
}

fn prepare_dep_info_output(path: &Path) -> Result<(), WrapperError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(path).map_err(|source| WrapperError::WriteDepInfo {
                path: path.to_path_buf(),
                source,
            })
        }
        Ok(_) => Err(WrapperError::UnsafeDepInfoOutput(path.to_path_buf())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(WrapperError::ReadDepInfo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_dep_info_output_path(target: &Path, path: &Path) -> Result<(), WrapperError> {
    if !target.is_absolute() || !path.is_absolute() {
        return Err(WrapperError::DepInfoOutsideTarget {
            path: path.to_path_buf(),
            target: target.to_path_buf(),
        });
    }
    validate_dep_info_path_spelling(target, false)?;
    validate_dep_info_path_spelling(path, true)?;
    let relative = path
        .strip_prefix(target)
        .map_err(|_| WrapperError::DepInfoOutsideTarget {
            path: path.to_path_buf(),
            target: target.to_path_buf(),
        })?;
    let mut components = relative.components().peekable();
    if components.peek().is_none() {
        return Err(WrapperError::DepInfoOutsideTarget {
            path: path.to_path_buf(),
            target: target.to_path_buf(),
        });
    }
    let target_metadata =
        fs::symlink_metadata(target).map_err(|source| WrapperError::ReadDepInfo {
            path: target.to_path_buf(),
            source,
        })?;
    if target_metadata.file_type().is_symlink() || !target_metadata.file_type().is_dir() {
        return Err(WrapperError::UnsafeDepInfoOutput(target.to_path_buf()));
    }
    let mut current = target.to_path_buf();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(component) = component else {
            return Err(WrapperError::DepInfoOutsideTarget {
                path: path.to_path_buf(),
                target: target.to_path_buf(),
            });
        };
        current.push(component);
        if components.peek().is_none() {
            break;
        }
        let metadata =
            fs::symlink_metadata(&current).map_err(|source| WrapperError::ReadDepInfo {
                path: current.clone(),
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(WrapperError::UnsafeDepInfoOutput(current));
        }
    }
    Ok(())
}

fn target_relative_dep_info_path(target: &Path, path: &Path) -> Result<String, WrapperError> {
    validate_dep_info_output_path(target, path)?;
    let relative = path
        .strip_prefix(target)
        .map_err(|_| WrapperError::DepInfoOutsideTarget {
            path: path.to_path_buf(),
            target: target.to_path_buf(),
        })?;
    portable_package_relative_path(relative).map_err(|source| WrapperError::ReadDepInfo {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_no_symlink_path(base: &Path, relative: &Path) -> Result<(), WrapperError> {
    let mut path = base.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(component) = component else {
            return Err(WrapperError::UnsafeOutDirDependency(base.join(relative)));
        };
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| WrapperError::ReadOutDirDependency {
                path: path.clone(),
                source,
            })?;
        let expected_type = if components.peek().is_some() {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        };
        if metadata.file_type().is_symlink() || !expected_type {
            return Err(WrapperError::UnsafeOutDirDependency(path));
        }
    }
    Ok(())
}

fn validate_dep_info_path_spelling(
    path: &Path,
    terminal_is_file: bool,
) -> Result<(), WrapperError> {
    let mut last = None;
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            continue;
        };
        let component = component
            .to_str()
            .ok_or_else(|| WrapperError::UnrepresentableDepInfoPath(path.to_path_buf()))?;
        if component.contains('\\')
            || component
                .chars()
                .any(|character| character.is_whitespace() && character != ' ')
        {
            return Err(WrapperError::UnrepresentableDepInfoPath(path.to_path_buf()));
        }
        last = Some(component);
    }
    if terminal_is_file && last.is_some_and(|component| component.ends_with(' ')) {
        return Err(WrapperError::UnrepresentableDepInfoPath(path.to_path_buf()));
    }
    Ok(())
}

fn descriptor_out_dir_coordinate(
    target_dir: &Path,
    out_dir: Option<&str>,
) -> Result<Option<String>, WrapperError> {
    let Some(out_dir) = out_dir else {
        return Ok(None);
    };
    let out_dir = Path::new(out_dir);
    if !out_dir.is_absolute() {
        return Err(WrapperError::RelativeOutDir(out_dir.to_path_buf()));
    }
    validate_dep_info_path_spelling(target_dir, false)?;
    validate_dep_info_path_spelling(out_dir, false)?;
    let target_metadata =
        fs::symlink_metadata(target_dir).map_err(|source| WrapperError::ReadOutDirDependency {
            path: target_dir.to_path_buf(),
            source,
        })?;
    if target_metadata.file_type().is_symlink() || !target_metadata.file_type().is_dir() {
        return Err(WrapperError::UnsafeOutDirDependency(
            target_dir.to_path_buf(),
        ));
    }
    let relative = out_dir.strip_prefix(target_dir).map_err(|_| {
        WrapperError::OutDirOutsideDescriptorTarget {
            out_dir: out_dir.to_path_buf(),
            target: target_dir.to_path_buf(),
        }
    })?;
    if relative.as_os_str().is_empty() {
        return Err(WrapperError::OutDirOutsideDescriptorTarget {
            out_dir: out_dir.to_path_buf(),
            target: target_dir.to_path_buf(),
        });
    }
    let portable = portable_package_relative_path(relative).map_err(|source| {
        WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        }
    })?;
    let mut current = target_dir.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(WrapperError::UnsafeOutDirDependency(out_dir.to_path_buf()));
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|source| {
            WrapperError::ReadOutDirDependency {
                path: current.clone(),
                source,
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(WrapperError::UnsafeOutDirDependency(current));
        }
    }
    let canonical_target =
        fs::canonicalize(target_dir).map_err(|source| WrapperError::ReadOutDirDependency {
            path: target_dir.to_path_buf(),
            source,
        })?;
    let canonical_out_dir =
        fs::canonicalize(out_dir).map_err(|source| WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        })?;
    if canonical_out_dir == canonical_target || !canonical_out_dir.starts_with(&canonical_target) {
        return Err(WrapperError::OutDirOutsideDescriptorTarget {
            out_dir: out_dir.to_path_buf(),
            target: target_dir.to_path_buf(),
        });
    }
    Ok(Some(portable))
}

fn validate_out_dir_dependency_records(
    dependencies: &[DescriptorOutDirDependency],
) -> Result<(), WrapperError> {
    let mut previous = None;
    for dependency in dependencies {
        validate_portable_relative_path(&dependency.relative_path).map_err(|reason| {
            WrapperError::InvalidOutDirDependencyPath {
                path: dependency.relative_path.clone(),
                reason,
            }
        })?;
        validate_lower_sha256(&dependency.content_hash).map_err(|reason| {
            WrapperError::InvalidSha256 {
                name: "OUT_DIR dependency content hash",
                value: dependency.content_hash.clone(),
                reason,
            }
        })?;
        if previous
            .as_ref()
            .is_some_and(|value: &String| value.as_str() >= dependency.relative_path.as_str())
        {
            return Err(WrapperError::NonCanonicalOutDirDependencies);
        }
        previous = Some(dependency.relative_path.clone());
    }
    Ok(())
}

fn validate_source_dependency_records(
    dependencies: &[DescriptorSourceDependency],
) -> Result<(), WrapperError> {
    let mut previous = None::<&Path>;
    for dependency in dependencies {
        if !dependency.absolute_path.is_absolute() {
            return Err(WrapperError::RelativeObservedSource(
                dependency.absolute_path.clone(),
            ));
        }
        validate_lower_sha256(&dependency.content_hash).map_err(|reason| {
            WrapperError::InvalidSha256 {
                name: "observed source dependency content hash",
                value: dependency.content_hash.clone(),
                reason,
            }
        })?;
        if previous.is_some_and(|value| value >= dependency.absolute_path.as_path()) {
            return Err(WrapperError::NonCanonicalSourceDependencies);
        }
        previous = Some(&dependency.absolute_path);
    }
    Ok(())
}

fn validate_environment_dependency_records(
    dependencies: &[DescriptorEnvironmentDependency],
) -> Result<(), WrapperError> {
    let mut previous = None::<&str>;
    for dependency in dependencies {
        if dependency.name.is_empty()
            || dependency.name.bytes().any(|byte| byte == 0)
            || wrapper_control_environment(&dependency.name)
        {
            return Err(WrapperError::InvalidEnvironmentDependencyName {
                name: dependency.name.clone(),
            });
        }
        if let Some(value_hash) = &dependency.value_hash {
            validate_lower_sha256(value_hash).map_err(|reason| WrapperError::InvalidSha256 {
                name: "observed environment dependency value hash",
                value: value_hash.clone(),
                reason,
            })?;
        }
        if previous.is_some_and(|value| value >= dependency.name.as_str()) {
            return Err(WrapperError::NonCanonicalEnvironmentDependencies);
        }
        previous = Some(&dependency.name);
    }
    Ok(())
}

fn validate_current_out_dir_dependencies(
    out_dir: Option<&Path>,
    expected: &[DescriptorOutDirDependency],
) -> Result<(), WrapperError> {
    validate_out_dir_dependency_records(expected)?;
    let Some(out_dir) = out_dir else {
        return if expected.is_empty() {
            Ok(())
        } else {
            Err(WrapperError::MissingOutDirForDependencies)
        };
    };
    if !out_dir.is_absolute() {
        return Err(WrapperError::RelativeOutDir(out_dir.to_path_buf()));
    }
    let out_dir =
        fs::canonicalize(out_dir).map_err(|source| WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        })?;
    if expected.is_empty() {
        return Ok(());
    }
    for dependency in expected {
        let mut path = out_dir.clone();
        for component in dependency.relative_path.split('/') {
            path.push(component);
            let metadata = fs::symlink_metadata(&path).map_err(|source| {
                WrapperError::ReadOutDirDependency {
                    path: path.clone(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() {
                return Err(WrapperError::UnsafeOutDirDependency(path));
            }
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| WrapperError::ReadOutDirDependency {
                path: path.clone(),
                source,
            })?;
        if !metadata.file_type().is_file() {
            return Err(WrapperError::UnsafeOutDirDependency(path));
        }
        let actual = hash_file(&path, OUT_DIR_FILE_HASH_DOMAIN).map_err(|source| {
            WrapperError::ReadOutDirDependency {
                path: path.clone(),
                source,
            }
        })?;
        let actual = hex_digest(&actual);
        if actual != dependency.content_hash {
            return Err(WrapperError::OutDirDependencyHashMismatch {
                path,
                expected: dependency.content_hash.clone(),
                actual,
            });
        }
    }
    Ok(())
}

/// Whether two rustc source-dependency sets describe the same consumed bytes.
///
/// The descriptor and final compilations run in separate tool-owned target
/// trees, so build-script outputs (sailfish's per-target template cache, for
/// example) keep identical bytes but get different generated file names in
/// each pass. Entries under the tool-owned `target/proa-rsjs` tree therefore
/// compare as a sorted multiset of content hashes; every path outside that
/// tree must still match exactly. `content_hash` is a domain-separated digest
/// of the exact bytes rustc read, so byte-for-byte identity of everything the
/// final compilation consumed is preserved. The relaxation deliberately does
/// not attempt to pair generated files across passes: their names share no
/// stable component, and hash-multiset equality is the strongest available
/// pairing (a content swap between two generated files with unchanged
/// multiset is accepted; both layouts were produced by the same trusted
/// build-script execution).
fn source_dependency_sets_equivalent(
    expected: &[DescriptorSourceDependency],
    actual: &[DescriptorSourceDependency],
) -> bool {
    fn under_tool_target(dependency: &DescriptorSourceDependency) -> bool {
        let components: Vec<_> = dependency.absolute_path.components().collect();
        components
            .windows(2)
            .any(|pair| pair[0].as_os_str() == "target" && pair[1].as_os_str() == "proa-rsjs")
    }
    fn split(
        dependencies: &[DescriptorSourceDependency],
    ) -> (Vec<&str>, Vec<&DescriptorSourceDependency>) {
        let mut generated_hashes = Vec::new();
        let mut exact = Vec::new();
        for dependency in dependencies {
            if under_tool_target(dependency) {
                generated_hashes.push(dependency.content_hash.as_str());
            } else {
                exact.push(dependency);
            }
        }
        generated_hashes.sort_unstable();
        exact.sort_unstable();
        (generated_hashes, exact)
    }
    let (expected_generated, expected_exact) = split(expected);
    let (actual_generated, actual_exact) = split(actual);
    expected_exact == actual_exact && expected_generated == actual_generated
}

fn validate_current_source_dependencies(
    expected: &[DescriptorSourceDependency],
) -> Result<(), WrapperError> {
    validate_source_dependency_records(expected)?;
    for dependency in expected {
        let metadata = fs::symlink_metadata(&dependency.absolute_path).map_err(|source| {
            WrapperError::ReadObservedSource {
                path: dependency.absolute_path.clone(),
                source,
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(WrapperError::UnsafeObservedSource(
                dependency.absolute_path.clone(),
            ));
        }
        let canonical = fs::canonicalize(&dependency.absolute_path).map_err(|source| {
            WrapperError::ReadObservedSource {
                path: dependency.absolute_path.clone(),
                source,
            }
        })?;
        if canonical != dependency.absolute_path {
            return Err(WrapperError::UnsafeObservedSource(
                dependency.absolute_path.clone(),
            ));
        }
        let actual = hash_file(&canonical, OBSERVED_SOURCE_FILE_HASH_DOMAIN).map_err(|source| {
            WrapperError::ReadObservedSource {
                path: canonical.clone(),
                source,
            }
        })?;
        let actual = hex_digest(&actual);
        if actual != dependency.content_hash {
            return Err(WrapperError::ObservedSourceHashMismatch {
                path: canonical,
                expected: dependency.content_hash.clone(),
                actual,
            });
        }
    }
    Ok(())
}

fn validate_current_environment_dependencies(
    environment: &BTreeMap<String, String>,
    out_dir: Option<&Path>,
    out_dir_coordinate: Option<&str>,
    expected: &[DescriptorEnvironmentDependency],
) -> Result<(), WrapperError> {
    validate_environment_dependency_records(expected)?;
    for dependency in expected {
        let actual_hash = match environment.get(&dependency.name) {
            None => None,
            Some(value) if dependency.name == "OUT_DIR" => {
                let expected_out_dir = out_dir.ok_or(WrapperError::InconsistentObservedOutDir)?;
                if Path::new(value) != expected_out_dir {
                    return Err(WrapperError::ObservedEnvironmentMismatch {
                        name: dependency.name.clone(),
                    });
                }
                let coordinate =
                    out_dir_coordinate.ok_or(WrapperError::InconsistentObservedOutDir)?;
                Some(observed_environment_value_hash(
                    &dependency.name,
                    b"out-dir-coordinate",
                    coordinate.as_bytes(),
                ))
            }
            Some(value) => Some(observed_environment_value_hash(
                &dependency.name,
                b"exact-value",
                value.as_bytes(),
            )),
        };
        if actual_hash != dependency.value_hash {
            return Err(WrapperError::ObservedEnvironmentMismatch {
                name: dependency.name.clone(),
            });
        }
    }
    Ok(())
}

fn validate_final_out_dir(
    final_target: &Path,
    out_dir: Option<&Path>,
) -> Result<Option<PathBuf>, WrapperError> {
    if !final_target.is_absolute() {
        return Err(WrapperError::RelativeFinalTarget(
            final_target.to_path_buf(),
        ));
    }
    validate_dep_info_path_spelling(final_target, false)?;
    let target_metadata = fs::symlink_metadata(final_target).map_err(|source| {
        WrapperError::ReadOutDirDependency {
            path: final_target.to_path_buf(),
            source,
        }
    })?;
    if !target_metadata.file_type().is_dir() || target_metadata.file_type().is_symlink() {
        return Err(WrapperError::UnsafeFinalTarget(final_target.to_path_buf()));
    }
    let Some(out_dir) = out_dir else {
        return Ok(None);
    };
    if !out_dir.is_absolute() {
        return Err(WrapperError::RelativeOutDir(out_dir.to_path_buf()));
    }
    validate_dep_info_path_spelling(out_dir, false)?;
    let relative =
        out_dir
            .strip_prefix(final_target)
            .map_err(|_| WrapperError::OutDirOutsideFinalTarget {
                out_dir: out_dir.to_path_buf(),
                target: final_target.to_path_buf(),
            })?;
    if relative.as_os_str().is_empty() {
        return Err(WrapperError::OutDirOutsideFinalTarget {
            out_dir: out_dir.to_path_buf(),
            target: final_target.to_path_buf(),
        });
    }
    portable_package_relative_path(relative).map_err(|source| {
        WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        }
    })?;
    let mut path = final_target.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(WrapperError::UnsafeOutDirDependency(out_dir.to_path_buf()));
        };
        path.push(component);
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| WrapperError::ReadOutDirDependency {
                path: path.clone(),
                source,
            })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(WrapperError::UnsafeOutDirDependency(path));
        }
    }
    let canonical_target =
        fs::canonicalize(final_target).map_err(|source| WrapperError::ReadOutDirDependency {
            path: final_target.to_path_buf(),
            source,
        })?;
    let canonical_out_dir =
        fs::canonicalize(out_dir).map_err(|source| WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        })?;
    if canonical_out_dir == canonical_target || !canonical_out_dir.starts_with(&canonical_target) {
        return Err(WrapperError::OutDirOutsideFinalTarget {
            out_dir: out_dir.to_path_buf(),
            target: final_target.to_path_buf(),
        });
    }
    Ok(Some(canonical_out_dir))
}

fn final_out_dir_coordinate(
    final_target: &Path,
    out_dir: Option<&Path>,
) -> Result<Option<String>, WrapperError> {
    let Some(out_dir) = out_dir else {
        return Ok(None);
    };
    let relative =
        out_dir
            .strip_prefix(final_target)
            .map_err(|_| WrapperError::OutDirOutsideFinalTarget {
                out_dir: out_dir.to_path_buf(),
                target: final_target.to_path_buf(),
            })?;
    if relative.as_os_str().is_empty() {
        return Err(WrapperError::OutDirOutsideFinalTarget {
            out_dir: out_dir.to_path_buf(),
            target: final_target.to_path_buf(),
        });
    }
    portable_package_relative_path(relative)
        .map(Some)
        .map_err(|source| WrapperError::ReadOutDirDependency {
            path: out_dir.to_path_buf(),
            source,
        })
}

pub(crate) fn validate_descriptor_observed_inputs(
    descriptor_target: &Path,
    sidecar: &DescriptorUnitSidecar,
) -> Result<(), WrapperError> {
    let unit = DescriptorCargoUnitId::from_hex(&sidecar.cargo_unit_id)?;
    validate_sidecar_out_dir_dependencies(unit, sidecar)
        .map_err(|error| WrapperError::InvalidCachedOutDirDependencies(error.to_string()))?;
    let target_metadata = fs::symlink_metadata(descriptor_target).map_err(|source| {
        WrapperError::ReadOutDirDependency {
            path: descriptor_target.to_path_buf(),
            source,
        }
    })?;
    if target_metadata.file_type().is_symlink() || !target_metadata.file_type().is_dir() {
        return Err(WrapperError::UnsafeOutDirDependency(
            descriptor_target.to_path_buf(),
        ));
    }
    let out_dir = if let Some(relative) = sidecar.out_dir.as_deref() {
        let mut path = descriptor_target.to_path_buf();
        for component in relative.split('/') {
            path.push(component);
            let metadata = fs::symlink_metadata(&path).map_err(|source| {
                WrapperError::ReadOutDirDependency {
                    path: path.clone(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                return Err(WrapperError::UnsafeOutDirDependency(path));
            }
        }
        Some(path)
    } else {
        None
    };
    let working_directory = canonical_plain_directory(&sidecar.dep_info_working_directory)?;
    if working_directory != sidecar.dep_info_working_directory {
        return Err(WrapperError::UnsafeObservedSource(
            sidecar.dep_info_working_directory.clone(),
        ));
    }
    let mut dep_info_path = descriptor_target.to_path_buf();
    for component in sidecar.dep_info_path.split('/') {
        dep_info_path.push(component);
    }
    validate_dep_info_output_path(descriptor_target, &dep_info_path)?;
    let mut descriptor_phase_environment = BTreeMap::new();
    descriptor_phase_environment.insert(CARGO_UNIT_ID_ENV.to_string(), Some(unit.to_string()));
    descriptor_phase_environment.insert(DESCRIPTOR_PHASE_ENV.to_string(), Some("1".to_string()));
    let observed = observe_rustc_inputs(
        &dep_info_path,
        &working_directory,
        out_dir.as_deref(),
        sidecar.out_dir.as_deref(),
        &descriptor_phase_environment,
    )?;
    if observed.out_dir_dependencies != sidecar.out_dir_dependencies
        || observed.source_dependencies != sidecar.source_dependencies
        || observed.environment_dependencies != sidecar.environment_dependencies
    {
        return Err(WrapperError::CachedObservedInputsMismatch { unit });
    }
    Ok(())
}

fn validate_portable_relative_path(path: &str) -> Result<(), String> {
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

fn parse_rustc_dep_info(bytes: &[u8]) -> Result<ParsedDepInfo, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| "dep-info contains a non-UTF-8 path".to_string())?;
    let mut main_dependencies = None::<String>;
    let mut phony_dependencies = Vec::<String>::new();
    let mut environment = BTreeMap::<String, Option<String>>::new();
    let mut saw_phony = false;
    let mut saw_metadata = false;

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.is_empty() {
            continue;
        }
        if let Some(raw) = line.strip_prefix("# env-dep:") {
            saw_metadata = true;
            let (name, value) = raw
                .split_once('=')
                .map_or((raw, None), |(name, value)| (name, Some(value)));
            let name = decode_rustc_dep_info_environment(name).map_err(|reason| {
                format!("invalid environment dependency name at line {line_number}: {reason}")
            })?;
            if name.is_empty() || name.bytes().any(|byte| byte == 0) {
                return Err(format!(
                    "invalid environment dependency name at line {line_number}"
                ));
            }
            let value = value
                .map(decode_rustc_dep_info_environment)
                .transpose()
                .map_err(|reason| {
                    format!("invalid environment dependency value at line {line_number}: {reason}")
                })?;
            if environment.insert(name.clone(), value).is_some() {
                return Err(format!(
                    "dep-info repeats environment dependency `{name}` at line {line_number}"
                ));
            }
            continue;
        }
        if line.starts_with("# checksum:") {
            saw_metadata = true;
            continue;
        }
        if saw_metadata {
            return Err(format!(
                "dep-info rule appears after metadata at line {line_number}"
            ));
        }
        if let Some((target, dependencies)) = line.rsplit_once(": ") {
            if saw_phony {
                return Err(format!(
                    "dep-info main rule appears after phony targets at line {line_number}"
                ));
            }
            if target.is_empty() || dependencies.is_empty() {
                return Err(format!(
                    "dep-info main rule has an empty target or dependency set at line {line_number}"
                ));
            }
            if let Some(expected) = &main_dependencies {
                if expected != dependencies {
                    return Err(format!(
                        "dep-info main rules disagree at line {line_number}"
                    ));
                }
            } else {
                main_dependencies = Some(dependencies.to_string());
            }
            continue;
        }
        if let Some(target) = line.strip_suffix(':') {
            saw_phony = true;
            if target.is_empty() {
                return Err(format!(
                    "dep-info has an empty phony target at line {line_number}"
                ));
            }
            phony_dependencies.push(target.to_string());
            continue;
        }
        return Err(format!("unrecognized dep-info line {line_number}"));
    }

    let main_dependencies =
        main_dependencies.ok_or_else(|| "dep-info contains no main rule".to_string())?;
    if phony_dependencies.is_empty() {
        return Err("dep-info contains no phony dependency targets".to_string());
    }
    if phony_dependencies.join(" ") != main_dependencies {
        return Err("dep-info main dependencies and phony targets disagree".to_string());
    }
    let decoded = phony_dependencies
        .into_iter()
        .map(|dependency| decode_rustc_dep_info_path(&dependency))
        .collect::<Result<Vec<_>, _>>()?;
    let canonical = decoded.iter().collect::<BTreeSet<_>>();
    if canonical.len() != decoded.len() {
        return Err("dep-info repeats a dependency target".to_string());
    }
    Ok(ParsedDepInfo {
        source_paths: decoded.into_iter().map(PathBuf::from).collect(),
        environment: environment
            .into_iter()
            .map(|(name, value)| ParsedEnvironmentDependency { name, value })
            .collect(),
    })
}

fn decode_rustc_dep_info_path(value: &str) -> Result<String, String> {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&' ') {
            characters.next();
            decoded.push(' ');
        } else {
            if character == ' ' {
                return Err("dep-info path contains an unescaped space".to_string());
            }
            if character.is_whitespace() && character != ' ' {
                return Err("dep-info path contains unrepresentable whitespace".to_string());
            }
            decoded.push(character);
        }
    }
    Ok(decoded)
}

fn decode_rustc_dep_info_environment(value: &str) -> Result<String, String> {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('\\') => decoded.push('\\'),
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some(_) => return Err("unsupported escaped character".to_string()),
            None => return Err("trailing escape".to_string()),
        }
    }
    Ok(decoded)
}

fn portable_package_relative_path(path: &Path) -> io::Result<String> {
    let mut output = String::new();
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "package-relative path is not normalized: `{}`",
                    path.display()
                ),
            ));
        };
        let component = component.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "package source closure contains a non-UTF-8 relative path",
            )
        })?;
        if component.contains('\\')
            || component
                .chars()
                .any(|character| character.is_whitespace() && character != ' ')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("path component is ambiguous in rustc dep-info: `{component}`"),
            ));
        }
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(component);
    }
    if output.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package source closure contains an empty relative path",
        ));
    }
    Ok(output)
}

fn hash_file(path: &Path, domain: &[u8]) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn hash_file_two_domains(
    path: &Path,
    first_domain: &[u8],
    second_domain: &[u8],
) -> io::Result<([u8; 32], [u8; 32])> {
    let mut file = File::open(path)?;
    let mut first = Sha256::new();
    first.update(first_domain);
    let mut second = Sha256::new();
    second.update(second_domain);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        first.update(&buffer[..read]);
        second.update(&buffer[..read]);
    }
    Ok((first.finalize().into(), second.finalize().into()))
}

#[cfg(test)]
pub(crate) fn descriptor_input_content_hash(path: &Path) -> io::Result<String> {
    hash_file(path, FILE_HASH_DOMAIN).map(|digest| hex_digest(&digest))
}

pub(crate) fn descriptor_input_link_hashes(path: &Path) -> io::Result<([u8; 32], [u8; 32])> {
    hash_file_two_domains(path, FILE_HASH_DOMAIN, LINK_ARTIFACT_HASH_DOMAIN)
}

pub(crate) type DescriptorInputHashes = BTreeMap<PathBuf, ([u8; 32], [u8; 32])>;

#[cfg(test)]
pub(crate) fn descriptor_out_dir_dependency_content_hash(path: &Path) -> io::Result<String> {
    hash_file(path, OUT_DIR_FILE_HASH_DOMAIN).map(|digest| hex_digest(&digest))
}

#[cfg(test)]
pub(crate) fn descriptor_source_dependency_content_hash(path: &Path) -> io::Result<String> {
    hash_file(path, OBSERVED_SOURCE_FILE_HASH_DOMAIN).map(|digest| hex_digest(&digest))
}

#[cfg(test)]
pub(crate) fn descriptor_environment_dependency_value_hash(name: &str, value: &str) -> String {
    observed_environment_value_hash(name, b"exact-value", value.as_bytes())
}

pub(crate) fn descriptor_package_source_closure_hash(
    package_root: &Path,
    excluded_subtree: &Path,
) -> io::Result<String> {
    hash_package_source_closure(package_root, Some(excluded_subtree))
        .map(|digest| hex_digest(&digest))
}

#[cfg(test)]
pub(crate) fn descriptor_object_content_hash(path: &Path) -> io::Result<String> {
    hash_file(path, DESCRIPTOR_OBJECT_HASH_DOMAIN).map(|digest| hex_digest(&digest))
}

/// Hash one exact Cargo-enumerated artifact using the link-manifest domain.
///
/// The linker uses this same operation before extraction so a host-local path
/// cannot be swapped after the root manifest was assembled.
pub fn link_artifact_content_hash(path: &Path) -> io::Result<SemanticHash> {
    hash_file(path, LINK_ARTIFACT_HASH_DOMAIN).map(SemanticHash::from_bytes)
}

fn hash_bytes(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}

fn exact_sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex_digest(&digest)
}

fn hash_canonical(domain: &[u8], f: impl FnOnce(&mut CanonicalHasher)) -> [u8; 32] {
    let mut hash = CanonicalHasher::new(domain);
    f(&mut hash);
    hash.finish()
}

struct CanonicalHasher(Sha256);

impl CanonicalHasher {
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

    fn optional(&mut self, name: &str, value: Option<&[u8]>) {
        match value {
            Some(value) => {
                self.field(&format!("{name}:some"), value);
            }
            None => self.field(&format!("{name}:none"), &[]),
        }
    }

    fn set<'a>(&mut self, name: &str, values: impl IntoIterator<Item = &'a [u8]>) {
        let values = values.into_iter().collect::<Vec<_>>();
        self.field(&format!("{name}:len"), &(values.len() as u64).to_be_bytes());
        for value in values {
            self.field(name, value);
        }
    }

    fn map(&mut self, name: &str, values: &BTreeMap<String, String>) {
        self.field(&format!("{name}:len"), &(values.len() as u64).to_be_bytes());
        for (key, value) in values {
            self.field(&format!("{name}:key"), key.as_bytes());
            self.field(&format!("{name}:value"), value.as_bytes());
        }
    }

    fn digest_map(&mut self, name: &str, values: &BTreeMap<String, [u8; 32]>) {
        self.field(&format!("{name}:len"), &(values.len() as u64).to_be_bytes());
        for (key, value) in values {
            self.field(&format!("{name}:key"), key.as_bytes());
            self.field(&format!("{name}:value"), value);
        }
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

fn hex_digest(digest: &[u8; 32]) -> String {
    use fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub(crate) fn atomic_write_exact(path: &Path, bytes: &[u8]) -> Result<(), FinalUnitMapError> {
    let parent = path
        .parent()
        .ok_or_else(|| FinalUnitMapError::PathWithoutParent(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| FinalUnitMapError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;

    let nonce = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| FinalUnitMapError::InvalidFilename(path.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| FinalUnitMapError::Write {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| FinalUnitMapError::Write {
            path: temporary.clone(),
            source,
        })?;
    drop(file);

    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            fs::remove_file(&temporary).map_err(|source| FinalUnitMapError::Write {
                path: temporary,
                source,
            })?;
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let existing = fs::read(path).map_err(|source| FinalUnitMapError::Write {
                path: path.to_path_buf(),
                source,
            })?;
            let _ = fs::remove_file(&temporary);
            if existing == bytes {
                Ok(())
            } else {
                Err(FinalUnitMapError::ConflictingContent(path.to_path_buf()))
            }
        }
        Err(source) => Err(FinalUnitMapError::Write {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), WrapperError> {
    let parent = path
        .parent()
        .ok_or_else(|| WrapperError::SidecarWithoutParent(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| WrapperError::CreateStagingDirectory {
        path: parent.to_path_buf(),
        source,
    })?;

    let nonce = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| WrapperError::InvalidSidecarFileName(path.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| WrapperError::WriteSidecar {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| WrapperError::WriteSidecar {
            path: temporary.clone(),
            source,
        })?;
    drop(file);

    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            fs::remove_file(&temporary).map_err(|source| WrapperError::WriteSidecar {
                path: temporary,
                source,
            })?;
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let existing = fs::read(path).map_err(|source| WrapperError::WriteSidecar {
                path: path.to_path_buf(),
                source,
            })?;
            let mut expected = bytes.to_vec();
            expected.push(b'\n');
            let _ = fs::remove_file(&temporary);
            if existing == expected {
                Ok(())
            } else {
                Err(WrapperError::ConflictingUnitSidecar(path.to_path_buf()))
            }
        }
        Err(source) => Err(WrapperError::WriteSidecar {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DescriptorUnitSidecar {
    /// Sidecar schema version.
    pub schema_version: u16,
    /// Lowercase-hex descriptor Cargo-unit ID.
    pub cargo_unit_id: String,
    /// Host-local Cargo manifest used only to join this sidecar to the exact
    /// compiler-artifact message from the same invocation.
    pub cargo_manifest_path: Option<PathBuf>,
    /// Cargo package name exposed to this rustc unit.
    pub cargo_package_name: Option<String>,
    /// Cargo package version exposed to this rustc unit.
    pub cargo_package_version: Option<String>,
    /// Cargo profile exposed to this rustc unit.
    pub cargo_profile: Option<String>,
    /// Phase-invariant hash of Cargo/rustc profile-affecting inputs.
    pub cargo_profile_fingerprint: String,
    /// Whether Cargo marked this unit as belonging to a selected root package.
    pub cargo_primary_package: bool,
    /// Cargo's exact `-C extra-filename` discriminator for this rustc unit.
    #[serde(default)]
    pub cargo_extra_filename: Option<String>,
    /// Rust crate name.
    pub crate_name: String,
    /// Normalized crate types.
    pub crate_types: Vec<String>,
    /// Effective target triple when rustc received one explicitly.
    pub target: Option<String>,
    /// Normalized rustc cfgs.
    pub cfgs: Vec<String>,
    /// Cargo/rustc feature cfgs.
    pub features: Vec<String>,
    /// Content hash of the exact primary source input.
    pub source_hash: String,
    /// Host-independent digest of the conservative package-local source
    /// closure used to derive this unit's immutable identity.
    pub package_source_closure_hash: String,
    /// Exact `--extern` inputs, keyed by extern name. Paths are host-local join
    /// coordinates; only their content hashes participate in semantic unit
    /// identity.
    pub dependency_artifacts: BTreeMap<String, DescriptorDependencyArtifact>,
    /// Hash of every normalized descriptor-phase rustc input.
    pub normalized_rustc_inputs_hash: String,
    /// Phase-invariant identity of the semantic Cargo/build-script environment.
    pub logical_environment_hash: String,
    /// Descriptor-target-relative dep-info path produced by this exact rustc
    /// invocation. The cache reopens this Cargo-authoritative file when Cargo
    /// reports the unit fresh.
    pub dep_info_path: String,
    /// Exact canonical directory against which rustc spelled relative dep-info
    /// source paths during descriptor compilation.
    pub dep_info_working_directory: PathBuf,
    /// Cargo-target-relative build-script output directory. This is a
    /// host-local cache coordinate and never enters the semantic unit key.
    pub out_dir: Option<String>,
    /// Exact rustc-declared source dependencies consumed beneath `OUT_DIR` by
    /// the successful descriptor compilation.
    pub out_dir_dependencies: Vec<DescriptorOutDirDependency>,
    /// Every other exact rustc-declared source dependency, retained under its
    /// canonical host-local absolute path.
    pub source_dependencies: Vec<DescriptorSourceDependency>,
    /// Every non-wrapper-owned environment dependency rustc declared. Values
    /// are domain-separated hashes; plaintext values never leave dep-info.
    pub environment_dependencies: Vec<DescriptorEnvironmentDependency>,
    /// Private generation fingerprint of the complete effective rustc
    /// environment, excluding only cargo-proa wrapper-control variables.
    /// Plaintext environment values are never persisted.
    pub realized_environment_hash: String,
    /// Descriptor-only codegen policy.
    pub descriptor_codegen: DescriptorCodegenSidecar,
    /// Staging-directory-relative exact object path.
    pub descriptor_object: String,
    /// Content hash of the exact descriptor object.
    pub descriptor_object_hash: String,
}

/// One exact rustc `--extern` input retained for Cargo-artifact joining.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DescriptorDependencyArtifact {
    /// Exact host-local path passed to rustc by Cargo.
    pub path: PathBuf,
    /// Descriptor-domain content hash captured before descriptor compilation.
    pub content_hash: String,
}

/// One rustc-declared source dependency beneath a build script's `OUT_DIR`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DescriptorOutDirDependency {
    /// Slash-separated path relative to the unit's exact `OUT_DIR`.
    pub relative_path: String,
    /// Domain-separated hash of the bytes read by rustc.
    pub content_hash: String,
}

/// One rustc-declared source dependency outside the unit's exact `OUT_DIR`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DescriptorSourceDependency {
    /// Canonical host-local absolute source path.
    pub absolute_path: PathBuf,
    /// Domain-separated hash of the exact bytes read after rustc completed.
    pub content_hash: String,
}

/// One environment variable tracked by rustc's dep-info protocol.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DescriptorEnvironmentDependency {
    /// Exact decoded environment-variable name.
    pub name: String,
    /// `None` means rustc observed the variable absent. `Some` is the
    /// domain-separated hash of its decoded value; an empty value is distinct
    /// from absence.
    pub value_hash: Option<String>,
}

/// Descriptor-only rustc policy recorded beside each exact unit object.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DescriptorCodegenSidecar {
    /// Effective codegen-unit policy; the carrier includes every emitted object.
    pub codegen_units: u32,
    /// Whether Cargo enabled incremental query and object reuse.
    pub incremental: bool,
    /// Retain otherwise-dead transport statics.
    pub link_dead_code: bool,
    /// Descriptor-pass LTO policy.
    pub lto: String,
    /// Descriptor-pass strip policy.
    pub strip: String,
}

impl DescriptorCodegenSidecar {
    pub(crate) fn is_supported(&self) -> bool {
        self.codegen_units > 0 && self.link_dead_code && self.lto == "off" && self.strip == "none"
    }
}

/// One validated wrapper sidecar and its exact descriptor object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedDescriptorUnit {
    /// Strong realized Cargo-unit identity.
    pub unit: DescriptorCargoUnitId,
    /// Parsed, version-checked sidecar.
    pub sidecar: DescriptorUnitSidecar,
    /// Exact sidecar path in invocation staging or an immutable cache slot.
    pub sidecar_path: PathBuf,
    /// Exact descriptor object whose content hash was validated.
    pub descriptor_object: PathBuf,
}

/// Phase-invariant identity used to join a final rustc invocation to one
/// descriptor Cargo unit.
///
/// This deliberately excludes target-directory paths, output paths,
/// dependency artifact paths, and the final plan's injected cfgs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct FinalUnitCoordinate {
    /// Exact Cargo manifest shared by descriptor and final compilation.
    pub cargo_manifest_path: PathBuf,
    /// Cargo package name.
    pub cargo_package_name: String,
    /// Cargo package version.
    pub cargo_package_version: String,
    /// Cargo profile name when Cargo exposed one directly to rustc.
    pub cargo_profile: Option<String>,
    /// Stable profile-affecting rustc/environment fingerprint.
    pub cargo_profile_fingerprint: String,
    /// Whether Cargo marked this unit as belonging to a selected root package.
    pub cargo_primary_package: bool,
    /// Cargo's exact phase-invariant rustc output discriminator. Selected
    /// primary bins and libraries may intentionally use an unhashed output.
    pub cargo_extra_filename: Option<String>,
    /// Rust crate name.
    pub crate_name: String,
    /// Normalized crate types.
    pub crate_types: Vec<String>,
    /// Effective explicit target triple.
    pub target: Option<String>,
    /// Cargo feature cfg values.
    pub features: Vec<String>,
    /// Descriptor-domain content hash of the exact primary source.
    pub source_hash: String,
    /// Semantic Cargo/build-script environment.
    pub logical_environment_hash: String,
    /// Normalized cfgs supplied by Cargo before wrapper-owned final-plan cfgs.
    pub cfgs: Vec<String>,
}

/// One exact descriptor-to-final Cargo-unit mapping.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FinalUnitMapEntry {
    /// Phase-invariant semantic coordinate.
    pub coordinate: FinalUnitCoordinate,
    /// Original descriptor Cargo-unit ID exposed to proc macros.
    pub cargo_unit_id: String,
    /// Exact generated-source inputs that must be present before final
    /// compilation and must be re-observed by final rustc dep-info.
    pub out_dir_dependencies: Vec<DescriptorOutDirDependency>,
    /// Exact non-`OUT_DIR` sources that must remain byte-identical and must be
    /// re-observed by final rustc dep-info.
    pub source_dependencies: Vec<DescriptorSourceDependency>,
    /// Redacted user/Cargo environment observations that must remain exact
    /// across descriptor and final compilation.
    pub environment_dependencies: Vec<DescriptorEnvironmentDependency>,
}

/// Content-addressed descriptor-to-final unit map consumed by `final-v2`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FinalUnitMap {
    /// External file discriminator.
    pub format: String,
    /// Exact schema version.
    pub schema_version: u16,
    /// Plan this mapping authorizes.
    pub plan_id: String,
    /// Tagged `sha256:<hex>` digest of the exact linked-plan bytes.
    pub plan_file_hash: String,
    /// Canonically sorted, unambiguous mappings.
    pub entries: Vec<FinalUnitMapEntry>,
}

/// Exact file written for a final Cargo invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenFinalUnitMap {
    /// Absolute content-addressed path.
    pub path: PathBuf,
    /// SHA-256 of the exact bytes at `path`.
    pub content_hash: String,
}

/// Build and atomically publish a final unit map from validated descriptor
/// sidecars.
pub fn write_content_addressed_final_unit_map(
    directory: &Path,
    plan_id: PlanId,
    plan_file_hash: &str,
    units: &[ValidatedDescriptorUnit],
) -> Result<WrittenFinalUnitMap, FinalUnitMapError> {
    if !directory.is_absolute() {
        return Err(FinalUnitMapError::RelativeDirectory(
            directory.to_path_buf(),
        ));
    }
    validate_tagged_sha256(plan_file_hash).map_err(|reason| {
        FinalUnitMapError::InvalidPlanFileHash {
            value: plan_file_hash.to_string(),
            reason,
        }
    })?;

    let mut entries = units
        .iter()
        .map(final_unit_map_entry)
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| {
        left.coordinate
            .cmp(&right.coordinate)
            .then_with(|| left.cargo_unit_id.cmp(&right.cargo_unit_id))
    });
    for pair in entries.windows(2) {
        if pair[0].coordinate == pair[1].coordinate {
            return Err(FinalUnitMapError::AmbiguousCoordinate {
                coordinate: pair[0].coordinate.clone(),
                first: pair[0].cargo_unit_id.clone(),
                second: pair[1].cargo_unit_id.clone(),
            });
        }
    }

    let unit_map = FinalUnitMap {
        format: FINAL_UNIT_MAP_FORMAT.to_string(),
        schema_version: FINAL_UNIT_MAP_SCHEMA_VERSION,
        plan_id: plan_id.to_string(),
        plan_file_hash: plan_file_hash.to_string(),
        entries,
    };
    let mut bytes = serde_json::to_vec_pretty(&unit_map).map_err(FinalUnitMapError::Serialize)?;
    bytes.push(b'\n');
    let content_hash = exact_sha256_hex(&bytes);
    let path = directory.join(format!("{content_hash}.final-unit-map.json"));
    atomic_write_exact(&path, &bytes)?;
    Ok(WrittenFinalUnitMap { path, content_hash })
}

/// Track each exact mapping independently in Cargo dep-info. The global map
/// remains validated before every compile; unrelated entry edits need not
/// invalidate a macro's coordinate lookup.
pub(crate) fn final_unit_tracking_inputs(
    units: &[ValidatedDescriptorUnit],
) -> Result<BTreeMap<OsString, OsString>, FinalUnitMapError> {
    units
        .iter()
        .map(|unit| {
            let entry = final_unit_map_entry(unit)?;
            let (name, value) = final_unit_entry_tracking_input(&entry)?;
            Ok((name.into(), value.into()))
        })
        .collect()
}

fn final_unit_entry_tracking_input(
    entry: &FinalUnitMapEntry,
) -> Result<(String, String), FinalUnitMapError> {
    let bytes = serde_json::to_vec(entry).map_err(FinalUnitMapError::Serialize)?;
    let mut hash = Sha256::new();
    hash.update(b"proa.rsjs.final-unit-input.v1\0");
    hash.update(&bytes);
    Ok((
        format!("{FINAL_UNIT_INPUT_ENV_PREFIX}{}", entry.cargo_unit_id),
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

fn final_unit_map_entry(
    unit: &ValidatedDescriptorUnit,
) -> Result<FinalUnitMapEntry, FinalUnitMapError> {
    let sidecar = &unit.sidecar;
    if sidecar.cargo_unit_id != unit.unit.to_string() {
        return Err(FinalUnitMapError::UnitIdMismatch {
            validated: unit.unit.to_string(),
            sidecar: sidecar.cargo_unit_id.clone(),
        });
    }
    let cargo_manifest_path = sidecar.cargo_manifest_path.clone().ok_or_else(|| {
        FinalUnitMapError::MissingCoordinateField {
            unit: unit.unit.to_string(),
            field: "cargo_manifest_path",
        }
    })?;
    if !cargo_manifest_path.is_absolute() {
        return Err(FinalUnitMapError::RelativeManifestPath {
            unit: unit.unit.to_string(),
            path: cargo_manifest_path,
        });
    }
    let cargo_package_name =
        required_sidecar_field(unit, "cargo_package_name", &sidecar.cargo_package_name)?;
    let cargo_package_version = required_sidecar_field(
        unit,
        "cargo_package_version",
        &sidecar.cargo_package_version,
    )?;
    validate_lower_sha256(&sidecar.cargo_profile_fingerprint).map_err(|reason| {
        FinalUnitMapError::InvalidProfileFingerprint {
            unit: unit.unit.to_string(),
            value: sidecar.cargo_profile_fingerprint.clone(),
            reason,
        }
    })?;
    let cargo_extra_filename = sidecar.cargo_extra_filename.clone();
    if let Some(value) = &cargo_extra_filename {
        validate_cargo_extra_filename(value).map_err(|reason| {
            FinalUnitMapError::InvalidCargoExtraFilename {
                unit: unit.unit.to_string(),
                value: value.clone(),
                reason,
            }
        })?;
    }
    let crate_types = canonical_sidecar_values(unit, "crate_types", &sidecar.crate_types)?;
    let cfgs = canonical_sidecar_values(unit, "cfgs", &sidecar.cfgs)?;
    if cfgs.iter().any(|cfg| is_final_plan_cfg(cfg)) {
        return Err(FinalUnitMapError::ReservedDescriptorCfg {
            unit: unit.unit.to_string(),
        });
    }
    let features = canonical_sidecar_values(unit, "features", &sidecar.features)?;
    let derived_features = cfg_features(cfgs.iter().map(String::as_str));
    if features != derived_features {
        return Err(FinalUnitMapError::FeatureCfgMismatch {
            unit: unit.unit.to_string(),
            features,
            derived: derived_features,
        });
    }
    validate_lower_sha256(&sidecar.source_hash).map_err(|reason| {
        FinalUnitMapError::InvalidSourceHash {
            unit: unit.unit.to_string(),
            value: sidecar.source_hash.clone(),
            reason,
        }
    })?;
    validate_lower_sha256(&sidecar.package_source_closure_hash).map_err(|reason| {
        FinalUnitMapError::InvalidPackageSourceClosureHash {
            unit: unit.unit.to_string(),
            value: sidecar.package_source_closure_hash.clone(),
            reason,
        }
    })?;
    validate_lower_sha256(&sidecar.logical_environment_hash).map_err(|reason| {
        FinalUnitMapError::InvalidLogicalEnvironmentHash {
            unit: unit.unit.to_string(),
            value: sidecar.logical_environment_hash.clone(),
            reason,
        }
    })?;

    Ok(FinalUnitMapEntry {
        coordinate: FinalUnitCoordinate {
            cargo_manifest_path,
            cargo_package_name,
            cargo_package_version,
            cargo_profile: sidecar.cargo_profile.clone(),
            cargo_profile_fingerprint: sidecar.cargo_profile_fingerprint.clone(),
            cargo_primary_package: sidecar.cargo_primary_package,
            cargo_extra_filename,
            crate_name: sidecar.crate_name.clone(),
            crate_types,
            target: sidecar.target.clone(),
            features,
            source_hash: sidecar.source_hash.clone(),
            logical_environment_hash: sidecar.logical_environment_hash.clone(),
            cfgs,
        },
        cargo_unit_id: unit.unit.to_string(),
        out_dir_dependencies: sidecar.out_dir_dependencies.clone(),
        source_dependencies: sidecar.source_dependencies.clone(),
        environment_dependencies: sidecar.environment_dependencies.clone(),
    })
}

fn required_sidecar_field(
    unit: &ValidatedDescriptorUnit,
    field: &'static str,
    value: &Option<String>,
) -> Result<String, FinalUnitMapError> {
    value
        .as_ref()
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| FinalUnitMapError::MissingCoordinateField {
            unit: unit.unit.to_string(),
            field,
        })
}

fn canonical_sidecar_values(
    unit: &ValidatedDescriptorUnit,
    field: &'static str,
    values: &[String],
) -> Result<Vec<String>, FinalUnitMapError> {
    let canonical = values.iter().cloned().collect::<BTreeSet<_>>();
    if canonical.len() != values.len() || canonical.iter().any(String::is_empty) {
        return Err(FinalUnitMapError::NonCanonicalCoordinateValues {
            unit: unit.unit.to_string(),
            field,
        });
    }
    Ok(canonical.into_iter().collect())
}

#[derive(Debug)]
struct LoadedFinalUnitMap {
    entries: BTreeMap<FinalUnitCoordinate, LoadedFinalUnit>,
}

impl LoadedFinalUnitMap {
    fn resolve(&self, coordinate: Option<&FinalUnitCoordinate>) -> Option<&LoadedFinalUnit> {
        coordinate.and_then(|coordinate| self.entries.get(coordinate))
    }
}

#[derive(Debug)]
struct LoadedFinalUnit {
    cargo_unit_id: DescriptorCargoUnitId,
    tracked_input: (String, String),
    out_dir_dependencies: Vec<DescriptorOutDirDependency>,
    source_dependencies: Vec<DescriptorSourceDependency>,
    environment_dependencies: Vec<DescriptorEnvironmentDependency>,
}

fn load_final_unit_map_from_env() -> Result<LoadedFinalUnitMap, WrapperError> {
    if env::var(FINAL_PHASE_ENV).as_deref() != Ok("1") {
        return Err(WrapperError::InvalidFinalPhase);
    }
    let expected_plan_id = env::var(FINAL_PLAN_ID_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_PLAN_ID_ENV))?;
    let expected_plan_file_hash = env::var(FINAL_PLAN_FILE_HASH_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_PLAN_FILE_HASH_ENV))?;
    validate_tagged_sha256(&expected_plan_file_hash).map_err(|reason| {
        WrapperError::InvalidSha256 {
            name: FINAL_PLAN_FILE_HASH_ENV,
            value: expected_plan_file_hash.clone(),
            reason,
        }
    })?;
    let path = env::var_os(FINAL_UNIT_MAP_PATH_ENV)
        .map(PathBuf::from)
        .ok_or(WrapperError::MissingEnvironment(FINAL_UNIT_MAP_PATH_ENV))?;
    if !path.is_absolute() {
        return Err(WrapperError::RelativeFinalUnitMap(path));
    }
    let expected_hash = env::var(FINAL_UNIT_MAP_HASH_ENV)
        .map_err(|_| WrapperError::MissingEnvironment(FINAL_UNIT_MAP_HASH_ENV))?;
    validate_lower_sha256(&expected_hash).map_err(|reason| WrapperError::InvalidSha256 {
        name: FINAL_UNIT_MAP_HASH_ENV,
        value: expected_hash.clone(),
        reason,
    })?;
    let expected_filename = format!("{expected_hash}.final-unit-map.json");
    if path.file_name() != Some(OsStr::new(&expected_filename)) {
        return Err(WrapperError::FinalUnitMapFilename {
            path,
            expected: expected_filename,
        });
    }
    let bytes = fs::read(&path).map_err(|source| WrapperError::ReadFinalUnitMap {
        path: path.clone(),
        source,
    })?;
    let actual_hash = exact_sha256_hex(&bytes);
    if actual_hash != expected_hash {
        return Err(WrapperError::FinalUnitMapHashMismatch {
            path,
            expected: expected_hash,
            actual: actual_hash,
        });
    }
    let unit_map: FinalUnitMap =
        serde_json::from_slice(&bytes).map_err(|source| WrapperError::ParseFinalUnitMap {
            path: path.clone(),
            source,
        })?;
    if unit_map.format != FINAL_UNIT_MAP_FORMAT {
        return Err(WrapperError::FinalUnitMapFormat(unit_map.format));
    }
    if unit_map.schema_version != FINAL_UNIT_MAP_SCHEMA_VERSION {
        return Err(WrapperError::FinalUnitMapSchema {
            actual: unit_map.schema_version,
            expected: FINAL_UNIT_MAP_SCHEMA_VERSION,
        });
    }
    if unit_map.plan_id != expected_plan_id {
        return Err(WrapperError::FinalUnitMapPlanId {
            expected: expected_plan_id,
            actual: unit_map.plan_id,
        });
    }
    if unit_map.plan_file_hash != expected_plan_file_hash {
        return Err(WrapperError::FinalUnitMapPlanFileHash {
            expected: expected_plan_file_hash,
            actual: unit_map.plan_file_hash,
        });
    }

    let mut entries = BTreeMap::new();
    let mut previous = None;
    for entry in unit_map.entries {
        validate_final_coordinate(&entry.coordinate)?;
        if previous
            .as_ref()
            .is_some_and(|coordinate| coordinate > &entry.coordinate)
        {
            return Err(WrapperError::NonCanonicalFinalUnitMap);
        }
        previous = Some(entry.coordinate.clone());
        validate_out_dir_dependency_records(&entry.out_dir_dependencies)?;
        validate_source_dependency_records(&entry.source_dependencies)?;
        validate_environment_dependency_records(&entry.environment_dependencies)?;
        let cargo_unit_id = DescriptorCargoUnitId::from_hex(&entry.cargo_unit_id)?;
        let tracked_input = final_unit_entry_tracking_input(&entry)
            .map_err(|error| WrapperError::DescriptorCarrier(error.to_string()))?;
        let loaded = LoadedFinalUnit {
            cargo_unit_id,
            tracked_input,
            out_dir_dependencies: entry.out_dir_dependencies,
            source_dependencies: entry.source_dependencies,
            environment_dependencies: entry.environment_dependencies,
        };
        if let Some(first) = entries.insert(entry.coordinate.clone(), loaded) {
            return Err(WrapperError::AmbiguousFinalUnitCoordinate {
                coordinate: entry.coordinate,
                first: first.cargo_unit_id.to_string(),
                second: cargo_unit_id.to_string(),
            });
        }
    }
    Ok(LoadedFinalUnitMap { entries })
}

fn validate_final_coordinate(coordinate: &FinalUnitCoordinate) -> Result<(), WrapperError> {
    if !coordinate.cargo_manifest_path.is_absolute() {
        return Err(WrapperError::RelativeMappedManifest(
            coordinate.cargo_manifest_path.clone(),
        ));
    }
    for (field, values) in [
        ("crate_types", &coordinate.crate_types),
        ("features", &coordinate.features),
        ("cfgs", &coordinate.cfgs),
    ] {
        let canonical = values.iter().cloned().collect::<BTreeSet<_>>();
        if canonical.len() != values.len()
            || canonical.iter().any(String::is_empty)
            || canonical.into_iter().collect::<Vec<_>>() != *values
        {
            return Err(WrapperError::NonCanonicalFinalCoordinate(field));
        }
    }
    if coordinate.cfgs.iter().any(|cfg| is_final_plan_cfg(cfg)) {
        return Err(WrapperError::FinalCoordinateContainsPlanCfg);
    }
    if coordinate
        .cargo_profile
        .as_ref()
        .is_some_and(String::is_empty)
    {
        return Err(WrapperError::EmptyFinalProfile);
    }
    if let Some(value) = &coordinate.cargo_extra_filename {
        validate_cargo_extra_filename(value).map_err(|reason| {
            WrapperError::InvalidCargoExtraFilename {
                context: "final unit-map coordinate",
                value: value.clone(),
                reason,
            }
        })?;
    }
    validate_lower_sha256(&coordinate.cargo_profile_fingerprint).map_err(|reason| {
        WrapperError::InvalidSha256 {
            name: "final unit cargo_profile_fingerprint",
            value: coordinate.cargo_profile_fingerprint.clone(),
            reason,
        }
    })?;
    let derived_features = cfg_features(coordinate.cfgs.iter().map(String::as_str));
    if derived_features != coordinate.features {
        return Err(WrapperError::FinalCoordinateFeatureMismatch);
    }
    validate_lower_sha256(&coordinate.source_hash).map_err(|reason| {
        WrapperError::InvalidSha256 {
            name: "final unit source_hash",
            value: coordinate.source_hash.clone(),
            reason,
        }
    })?;
    validate_lower_sha256(&coordinate.logical_environment_hash).map_err(|reason| {
        WrapperError::InvalidSha256 {
            name: "final unit logical_environment_hash",
            value: coordinate.logical_environment_hash.clone(),
            reason,
        }
    })
}

struct FinalInvocationCoordinate {
    coordinate: FinalUnitCoordinate,
    input: NormalizedRustcInvocation,
}

fn final_unit_coordinate_from_invocation(
    rustc_args: &[OsString],
) -> Result<Option<FinalInvocationCoordinate>, WrapperError> {
    let cargo_manifest_dir = match env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        Some(directory) if directory.is_absolute() => directory,
        _ => return Ok(None),
    };
    let source_exclusion = env::var_os(SOURCE_EXCLUSION_DIR_ENV)
        .map(PathBuf::from)
        .ok_or(WrapperError::MissingEnvironment(SOURCE_EXCLUSION_DIR_ENV))?;
    if !source_exclusion.is_absolute() {
        return Err(WrapperError::RelativeSourceExclusionDirectory(
            source_exclusion,
        ));
    }
    let environment = rustc_visible_environment()?;
    let input = match normalize_rustc_invocation_with_package_root(
        rustc_args,
        &environment,
        b"final-coordinate-does-not-use-rustc-identity",
        Some(&cargo_manifest_dir),
        Some(&source_exclusion),
    ) {
        Ok(input) => input,
        Err(_) => return Ok(None),
    };
    let cargo_manifest_path = cargo_manifest_dir.join("Cargo.toml");
    let Some(cargo_package_name) = nonempty_env("CARGO_PKG_NAME") else {
        return Ok(None);
    };
    let Some(cargo_package_version) = nonempty_env("CARGO_PKG_VERSION") else {
        return Ok(None);
    };
    let cargo_profile_fingerprint = hex_digest(&input.final_match_profile_hash());
    let cargo_extra_filename =
        input
            .cargo_extra_filename()
            .map_err(|reason| WrapperError::InvalidCargoExtraFilename {
                context: "final rustc invocation",
                value: input
                    .codegen_options
                    .get("extra-filename")
                    .cloned()
                    .unwrap_or_default(),
                reason,
            })?;
    let logical_environment_hash = hex_digest(&input.logical_environment_hash());
    let cfgs = validate_cargo_owned_final_coordinate_cfgs(input.cfgs.clone())?;
    let features = cfg_features(cfgs.iter().map(String::as_str));

    let coordinate = FinalUnitCoordinate {
        cargo_manifest_path,
        cargo_package_name,
        cargo_package_version,
        cargo_profile: nonempty_env("PROFILE"),
        cargo_profile_fingerprint,
        cargo_primary_package: env::var_os("CARGO_PRIMARY_PACKAGE").is_some(),
        cargo_extra_filename,
        crate_name: input.crate_name.clone(),
        crate_types: input.crate_types.iter().cloned().collect(),
        target: input.target.clone(),
        features,
        source_hash: hex_digest(&input.source_hash),
        logical_environment_hash,
        cfgs: cfgs.into_iter().collect(),
    };
    Ok(Some(FinalInvocationCoordinate { coordinate, input }))
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn validate_cargo_owned_final_coordinate_cfgs(
    cfgs: BTreeSet<String>,
) -> Result<BTreeSet<String>, WrapperError> {
    if let Some(cfg) = cfgs.iter().find(|cfg| is_final_plan_cfg(cfg)) {
        return Err(WrapperError::UnexpectedFinalPlanCfg(cfg.clone()));
    }
    Ok(cfgs)
}

fn is_final_plan_cfg(cfg: &str) -> bool {
    cfg == "rsjs_final"
        || cfg.starts_with("rsjs_final_wrapper_policy")
        || cfg.starts_with("rsjs_plan_id")
        || cfg.starts_with("rsjs_plan_file_hash")
        || cfg.starts_with("rsjs_final_unit_map_hash")
}

fn cfg_features<'a>(cfgs: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    cfgs.into_iter()
        .filter_map(|cfg| {
            cfg.strip_prefix("feature=\"")
                .and_then(|feature| feature.strip_suffix('"'))
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_lower_sha256(value: &str) -> Result<(), String> {
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

fn validate_tagged_sha256(value: &str) -> Result<(), String> {
    let digest = value
        .strip_prefix("sha256:")
        .ok_or_else(|| "expected `sha256:` prefix".to_string())?;
    validate_lower_sha256(digest)
}

fn validate_cargo_extra_filename(value: &str) -> Result<(), String> {
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

/// Load only freshly enumerated wrapper outputs from one explicit staging
/// directory.
///
/// This is not target-directory discovery. The orchestrator creates a unique
/// directory for one descriptor invocation and supplies that exact path to
/// both the wrapper and this loader.
pub fn load_descriptor_unit_sidecars(
    staging_dir: &Path,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorSidecarError> {
    load_descriptor_unit_sidecars_with_object_validation(
        staging_dir,
        DescriptorObjectValidation::HashBytes,
        false,
    )
}

pub(crate) fn load_descriptor_unit_sidecars_for_recovery(
    staging_dir: &Path,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorSidecarError> {
    load_descriptor_unit_sidecars_with_object_validation(
        staging_dir,
        DescriptorObjectValidation::HashBytes,
        true,
    )
}

enum DescriptorObjectValidation {
    HashBytes,
    TrustAtomicWrapperSidecar,
}

fn validate_sidecar_out_dir_dependencies(
    unit: DescriptorCargoUnitId,
    sidecar: &DescriptorUnitSidecar,
) -> Result<(), DescriptorSidecarError> {
    validate_portable_relative_path(&sidecar.dep_info_path).map_err(|reason| {
        DescriptorSidecarError::InvalidObservedInputs {
            unit,
            reason: format!("invalid target-relative dep-info path: {reason}"),
        }
    })?;
    if !sidecar.dep_info_working_directory.is_absolute() {
        return Err(DescriptorSidecarError::InvalidObservedInputs {
            unit,
            reason: "dep-info working directory is relative".to_string(),
        });
    }
    match &sidecar.out_dir {
        Some(path) => validate_portable_relative_path(path).map_err(|reason| {
            DescriptorSidecarError::InvalidOutDirPath {
                unit,
                path: path.clone(),
                reason,
            }
        })?,
        None if !sidecar.out_dir_dependencies.is_empty() => {
            return Err(DescriptorSidecarError::OutDirDependenciesWithoutOutDir(
                unit,
            ));
        }
        None => {}
    }
    let mut previous = None;
    for dependency in &sidecar.out_dir_dependencies {
        validate_portable_relative_path(&dependency.relative_path).map_err(|reason| {
            DescriptorSidecarError::InvalidOutDirDependencyPath {
                unit,
                path: dependency.relative_path.clone(),
                reason,
            }
        })?;
        validate_lower_sha256(&dependency.content_hash).map_err(|reason| {
            DescriptorSidecarError::InvalidOutDirDependencyHash {
                unit,
                path: dependency.relative_path.clone(),
                value: dependency.content_hash.clone(),
                reason,
            }
        })?;
        if previous
            .as_ref()
            .is_some_and(|value: &String| value.as_str() >= dependency.relative_path.as_str())
        {
            return Err(DescriptorSidecarError::NonCanonicalOutDirDependencies(unit));
        }
        previous = Some(dependency.relative_path.clone());
    }
    validate_source_dependency_records(&sidecar.source_dependencies).map_err(|error| {
        DescriptorSidecarError::InvalidObservedInputs {
            unit,
            reason: error.to_string(),
        }
    })?;
    validate_environment_dependency_records(&sidecar.environment_dependencies).map_err(
        |error| DescriptorSidecarError::InvalidObservedInputs {
            unit,
            reason: error.to_string(),
        },
    )?;
    if sidecar.out_dir.is_none()
        && sidecar
            .environment_dependencies
            .iter()
            .any(|dependency| dependency.name == "OUT_DIR" && dependency.value_hash.is_some())
    {
        return Err(DescriptorSidecarError::InvalidObservedInputs {
            unit,
            reason: "present OUT_DIR environment dependency has no OUT_DIR coordinate".to_string(),
        });
    }
    Ok(())
}

fn load_descriptor_unit_sidecars_trusting_wrapper_hashes(
    staging_dir: &Path,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorSidecarError> {
    load_descriptor_unit_sidecars_with_object_validation(
        staging_dir,
        DescriptorObjectValidation::TrustAtomicWrapperSidecar,
        false,
    )
}

fn load_descriptor_unit_sidecars_with_object_validation(
    staging_dir: &Path,
    object_validation: DescriptorObjectValidation,
    allow_interrupted_temporaries: bool,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorSidecarError> {
    if !staging_dir.is_absolute() {
        return Err(DescriptorSidecarError::RelativeStagingDirectory(
            staging_dir.to_path_buf(),
        ));
    }
    let units_dir = staging_dir.join("units");
    let entries =
        fs::read_dir(&units_dir).map_err(|source| DescriptorSidecarError::ReadUnitsDirectory {
            path: units_dir.clone(),
            source,
        })?;
    let mut sidecar_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| DescriptorSidecarError::ReadUnitsDirectory {
            path: units_dir.clone(),
            source,
        })?;
        let path = entry.path();
        if allow_interrupted_temporaries
            && interrupted_sidecar_temporary(entry.file_name().as_os_str())
        {
            let file_type =
                entry
                    .file_type()
                    .map_err(|source| DescriptorSidecarError::ReadUnitsDirectory {
                        path: units_dir.clone(),
                        source,
                    })?;
            if !file_type.is_file() {
                return Err(DescriptorSidecarError::UnexpectedUnitsEntry(path));
            }
            continue;
        }
        sidecar_paths.push(path);
    }
    sidecar_paths.sort();
    if sidecar_paths.is_empty() {
        return Err(DescriptorSidecarError::NoUnitSidecars(units_dir));
    }

    let mut seen = BTreeSet::new();
    let mut units = Vec::with_capacity(sidecar_paths.len());
    for sidecar_path in sidecar_paths {
        if sidecar_path.extension() != Some(OsStr::new("json")) {
            return Err(DescriptorSidecarError::UnexpectedUnitsEntry(sidecar_path));
        }
        let bytes =
            fs::read(&sidecar_path).map_err(|source| DescriptorSidecarError::ReadSidecar {
                path: sidecar_path.clone(),
                source,
            })?;
        let sidecar: DescriptorUnitSidecar = serde_json::from_slice(&bytes).map_err(|source| {
            DescriptorSidecarError::ParseSidecar {
                path: sidecar_path.clone(),
                source,
            }
        })?;
        if sidecar.schema_version != DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION {
            return Err(DescriptorSidecarError::UnsupportedSchema {
                path: sidecar_path,
                actual: sidecar.schema_version,
                expected: DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
            });
        }
        let unit = DescriptorCargoUnitId::from_hex(&sidecar.cargo_unit_id).map_err(|source| {
            DescriptorSidecarError::InvalidUnitId {
                path: sidecar_path.clone(),
                source,
            }
        })?;
        if !seen.insert(unit) {
            return Err(DescriptorSidecarError::DuplicateUnit(unit));
        }

        let expected_filename = format!("{unit}.json");
        if sidecar_path.file_name() != Some(OsStr::new(&expected_filename)) {
            return Err(DescriptorSidecarError::SidecarFilenameMismatch {
                path: sidecar_path,
                expected: expected_filename,
            });
        }
        if sidecar.normalized_rustc_inputs_hash != unit.to_string() {
            return Err(DescriptorSidecarError::NormalizedHashMismatch {
                unit,
                actual: sidecar.normalized_rustc_inputs_hash.clone(),
            });
        }
        validate_lower_sha256(&sidecar.logical_environment_hash).map_err(|reason| {
            DescriptorSidecarError::InvalidLogicalEnvironmentHash {
                unit,
                value: sidecar.logical_environment_hash.clone(),
                reason,
            }
        })?;
        validate_sidecar_out_dir_dependencies(unit, &sidecar)?;
        validate_lower_sha256(&sidecar.realized_environment_hash).map_err(|reason| {
            DescriptorSidecarError::InvalidRealizedEnvironmentHash {
                unit,
                value: sidecar.realized_environment_hash.clone(),
                reason,
            }
        })?;
        validate_lower_sha256(&sidecar.package_source_closure_hash).map_err(|reason| {
            DescriptorSidecarError::InvalidPackageSourceClosureHash {
                unit,
                value: sidecar.package_source_closure_hash.clone(),
                reason,
            }
        })?;
        for (extern_name, artifact) in &sidecar.dependency_artifacts {
            if extern_name.is_empty() {
                return Err(DescriptorSidecarError::EmptyDependencyName(unit));
            }
            if !artifact.path.is_absolute() {
                return Err(DescriptorSidecarError::RelativeDependencyArtifact {
                    unit,
                    extern_name: extern_name.clone(),
                    path: artifact.path.clone(),
                });
            }
            validate_lower_sha256(&artifact.content_hash).map_err(|reason| {
                DescriptorSidecarError::InvalidDependencyArtifactHash {
                    unit,
                    extern_name: extern_name.clone(),
                    value: artifact.content_hash.clone(),
                    reason,
                }
            })?;
        }
        if let Some(value) = &sidecar.cargo_extra_filename {
            validate_cargo_extra_filename(value).map_err(|reason| {
                DescriptorSidecarError::InvalidCargoExtraFilename {
                    unit,
                    value: value.clone(),
                    reason,
                }
            })?;
        }
        let expected_object = format!("objects/{unit}.o");
        if sidecar.descriptor_object != expected_object {
            return Err(DescriptorSidecarError::ObjectPathMismatch {
                unit,
                actual: sidecar.descriptor_object.clone(),
                expected: expected_object,
            });
        }
        if !sidecar.descriptor_codegen.is_supported() {
            return Err(DescriptorSidecarError::CodegenPolicyMismatch(unit));
        }
        validate_lower_sha256(&sidecar.descriptor_object_hash).map_err(|reason| {
            DescriptorSidecarError::InvalidDescriptorObjectHash {
                unit,
                value: sidecar.descriptor_object_hash.clone(),
                reason,
            }
        })?;
        let descriptor_object = staging_dir.join(&sidecar.descriptor_object);
        match &object_validation {
            DescriptorObjectValidation::HashBytes => {
                let actual_hash = hash_file(&descriptor_object, DESCRIPTOR_OBJECT_HASH_DOMAIN)
                    .map_err(|source| DescriptorSidecarError::ReadObject {
                        path: descriptor_object.clone(),
                        source,
                    })?;
                let actual_hash = hex_digest(&actual_hash);
                if actual_hash != sidecar.descriptor_object_hash {
                    return Err(DescriptorSidecarError::ObjectHashMismatch {
                        unit,
                        expected: sidecar.descriptor_object_hash.clone(),
                        actual: actual_hash,
                    });
                }
            }
            DescriptorObjectValidation::TrustAtomicWrapperSidecar => {
                require_regular_descriptor_object(&descriptor_object)?;
            }
        }

        units.push(ValidatedDescriptorUnit {
            unit,
            sidecar,
            sidecar_path,
            descriptor_object,
        });
    }
    Ok(units)
}

fn interrupted_sidecar_temporary(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(name) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((name, nonce)) = name.rsplit_once('.') else {
        return false;
    };
    let Some((sidecar, process)) = name.rsplit_once('.') else {
        return false;
    };
    let Some(unit) = sidecar.strip_suffix(".json") else {
        return false;
    };
    !process.is_empty()
        && process.bytes().all(|byte| byte.is_ascii_digit())
        && !nonce.is_empty()
        && nonce.bytes().all(|byte| byte.is_ascii_digit())
        && DescriptorCargoUnitId::from_hex(unit).is_ok()
}

fn require_regular_descriptor_object(path: &Path) -> Result<(), DescriptorSidecarError> {
    let metadata = fs::metadata(path).map_err(|source| DescriptorSidecarError::ReadObject {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(DescriptorSidecarError::ObjectNotRegularFile(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

/// Validation failure for one explicit descriptor staging directory.
#[derive(Debug, Error)]
pub enum DescriptorSidecarError {
    /// The orchestrator must resolve staging before launching Cargo.
    #[error("descriptor staging directory must be absolute: `{0}`")]
    RelativeStagingDirectory(PathBuf),
    /// Unit enumeration failed.
    #[error("failed to enumerate descriptor unit sidecars in `{path}`: {source}")]
    ReadUnitsDirectory {
        /// Exact units directory.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A successful descriptor invocation emitted no units.
    #[error("descriptor staging directory `{0}` contains no unit sidecars")]
    NoUnitSidecars(PathBuf),
    /// Fresh staging may contain only completed JSON sidecars.
    #[error("unexpected entry in descriptor units directory: `{0}`")]
    UnexpectedUnitsEntry(PathBuf),
    /// Sidecar bytes could not be read.
    #[error("failed to read descriptor sidecar `{path}`: {source}")]
    ReadSidecar {
        /// Exact sidecar path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Sidecar JSON could not be decoded.
    #[error("failed to parse descriptor sidecar `{path}`: {source}")]
    ParseSidecar {
        /// Exact sidecar path.
        path: PathBuf,
        /// JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// Sidecar schema is stale or from a future wrapper.
    #[error(
        "descriptor sidecar `{path}` uses schema {actual}; this tool requires schema {expected}"
    )]
    UnsupportedSchema {
        /// Exact sidecar path.
        path: PathBuf,
        /// Actual schema.
        actual: u16,
        /// Supported schema.
        expected: u16,
    },
    /// Unit ID text is malformed.
    #[error("descriptor sidecar `{path}` has an invalid Cargo-unit ID: {source}")]
    InvalidUnitId {
        /// Exact sidecar path.
        path: PathBuf,
        /// ID validation failure.
        #[source]
        source: WrapperError,
    },
    /// Two sidecars claimed one unit.
    #[error("descriptor staging contains duplicate Cargo unit {0}")]
    DuplicateUnit(DescriptorCargoUnitId),
    /// Filename and payload identity diverged.
    #[error("descriptor sidecar `{path}` must be named `{expected}`")]
    SidecarFilenameMismatch {
        /// Actual path.
        path: PathBuf,
        /// Required filename.
        expected: String,
    },
    /// The normalized-input checksum must equal the assigned unit ID.
    #[error(
        "descriptor unit {unit} reports normalized input hash `{actual}` instead of its unit ID"
    )]
    NormalizedHashMismatch {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Sidecar hash.
        actual: String,
    },
    /// Realized generation identity must use canonical lowercase SHA-256.
    #[error("descriptor unit {unit} has invalid realized environment hash `{value}`: {reason}")]
    InvalidRealizedEnvironmentHash {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cross-phase logical environment identity must be canonical SHA-256.
    #[error("descriptor unit {unit} has invalid logical environment hash `{value}`: {reason}")]
    InvalidLogicalEnvironmentHash {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// OUT_DIR is stored relative to the orchestrator-owned target.
    #[error("descriptor unit {unit} has invalid target-relative OUT_DIR `{path}`: {reason}")]
    InvalidOutDirPath {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected path.
        path: String,
        /// Validation detail.
        reason: String,
    },
    /// Generated dependencies require one matching OUT_DIR coordinate.
    #[error("descriptor unit {0} has OUT_DIR dependencies but no OUT_DIR coordinate")]
    OutDirDependenciesWithoutOutDir(DescriptorCargoUnitId),
    /// Generated-source paths are strict portable relative paths.
    #[error("descriptor unit {unit} has invalid OUT_DIR dependency path `{path}`: {reason}")]
    InvalidOutDirDependencyPath {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected path.
        path: String,
        /// Validation detail.
        reason: String,
    },
    /// Generated-source hashes use canonical lowercase SHA-256.
    #[error(
        "descriptor unit {unit} OUT_DIR dependency `{path}` has invalid hash `{value}`: {reason}"
    )]
    InvalidOutDirDependencyHash {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Generated-source path.
        path: String,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Generated-source records are strictly sorted and unique.
    #[error("descriptor unit {0} has non-canonical OUT_DIR dependencies")]
    NonCanonicalOutDirDependencies(DescriptorCargoUnitId),
    /// Exact dep-info/source/environment records failed closed validation.
    #[error("descriptor unit {unit} has invalid observed inputs: {reason}")]
    InvalidObservedInputs {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Value-redacted validation detail.
        reason: String,
    },
    /// Package closure identity must use the exact lowercase representation.
    #[error("descriptor unit {unit} has invalid package source-closure hash `{value}`: {reason}")]
    InvalidPackageSourceClosureHash {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Rust extern names are required map keys.
    #[error("descriptor unit {0} contains an empty dependency name")]
    EmptyDependencyName(DescriptorCargoUnitId),
    /// Cargo passes absolute artifact paths to rustc; relative coordinates
    /// cannot be joined safely to its JSON enumeration.
    #[error("descriptor unit {unit} extern `{extern_name}` has relative artifact path `{path}`")]
    RelativeDependencyArtifact {
        /// Descriptor unit.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
        /// Invalid path.
        path: PathBuf,
    },
    /// Wrapper-time dependency hashes must be canonical lowercase SHA-256.
    #[error(
        "descriptor unit {unit} extern `{extern_name}` has invalid artifact hash `{value}`: {reason}"
    )]
    InvalidDependencyArtifactHash {
        /// Descriptor unit.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
        /// Invalid hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cargo output discriminators must remain a single safe filename fragment.
    #[error("descriptor unit {unit} has invalid Cargo extra-filename `{value}`: {reason}")]
    InvalidCargoExtraFilename {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected discriminator.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Descriptor object path is not the content-addressed wrapper path.
    #[error("descriptor unit {unit} reports object `{actual}`; expected exact path `{expected}`")]
    ObjectPathMismatch {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Reported relative path.
        actual: String,
        /// Required relative path.
        expected: String,
    },
    /// Retention policy differs from the wrapper protocol.
    #[error("descriptor unit {0} has an incompatible retention/codegen policy")]
    CodegenPolicyMismatch(DescriptorCargoUnitId),
    /// Wrapper/cache object hashes must use canonical lowercase SHA-256.
    #[error("descriptor unit {unit} has invalid descriptor object hash `{value}`: {reason}")]
    InvalidDescriptorObjectHash {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Exact object could not be read.
    #[error("failed to read descriptor object `{path}`: {source}")]
    ReadObject {
        /// Exact object path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Object content differs from the atomic sidecar.
    #[error(
        "descriptor object for unit {unit} has hash `{actual}`; sidecar requires `{expected}`"
    )]
    ObjectHashMismatch {
        /// Assigned unit.
        unit: DescriptorCargoUnitId,
        /// Sidecar hash.
        expected: String,
        /// Recomputed hash.
        actual: String,
    },
    /// Skipping a byte rehash still requires a regular object file.
    #[error("descriptor object is not a regular file: `{0}`")]
    ObjectNotRegularFile(PathBuf),
}

/// Failure to build a phase-invariant final unit map.
#[derive(Debug, Error)]
pub enum FinalUnitMapError {
    /// Output location must not depend on the process cwd.
    #[error("final unit-map directory must be absolute: `{0}`")]
    RelativeDirectory(PathBuf),
    /// The exact tagged linked-plan hash was malformed.
    #[error("invalid linked-plan SHA-256 `{value}`: {reason}")]
    InvalidPlanFileHash {
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// A validated wrapper identity and its sidecar diverged.
    #[error("validated descriptor unit `{validated}` has sidecar Cargo-unit ID `{sidecar}`")]
    UnitIdMismatch {
        /// Validated ID.
        validated: String,
        /// Sidecar ID.
        sidecar: String,
    },
    /// A required Cargo coordinate was not recorded.
    #[error("descriptor unit {unit} has no nonempty `{field}` coordinate")]
    MissingCoordinateField {
        /// Descriptor unit.
        unit: String,
        /// Missing field.
        field: &'static str,
    },
    /// Profile matching requires one exact phase-invariant fingerprint.
    #[error("descriptor unit {unit} has invalid profile fingerprint `{value}`: {reason}")]
    InvalidProfileFingerprint {
        /// Descriptor unit.
        unit: String,
        /// Rejected fingerprint.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// The Cargo rustc output discriminator was malformed.
    #[error("descriptor unit {unit} has invalid Cargo extra-filename `{value}`: {reason}")]
    InvalidCargoExtraFilename {
        /// Descriptor unit.
        unit: String,
        /// Rejected discriminator.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Manifest paths are exact host-local join coordinates.
    #[error("descriptor unit {unit} has relative manifest path `{path}`")]
    RelativeManifestPath {
        /// Descriptor unit.
        unit: String,
        /// Rejected path.
        path: PathBuf,
    },
    /// Set-valued sidecar coordinates must already be canonical.
    #[error("descriptor unit {unit} has non-canonical `{field}` values")]
    NonCanonicalCoordinateValues {
        /// Descriptor unit.
        unit: String,
        /// Field name.
        field: &'static str,
    },
    /// Descriptor inputs may not claim final-only cfgs.
    #[error("descriptor unit {unit} contains a reserved final-plan cfg")]
    ReservedDescriptorCfg {
        /// Descriptor unit.
        unit: String,
    },
    /// Feature and cfg views must describe the same Cargo unit.
    #[error("descriptor unit {unit} has features {features:?}, but its cfgs derive {derived:?}")]
    FeatureCfgMismatch {
        /// Descriptor unit.
        unit: String,
        /// Recorded features.
        features: Vec<String>,
        /// Features derived from cfgs.
        derived: Vec<String>,
    },
    /// Source content identity must use the exact lowercase representation.
    #[error("descriptor unit {unit} has invalid source hash `{value}`: {reason}")]
    InvalidSourceHash {
        /// Descriptor unit.
        unit: String,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Package closure identity must use the exact lowercase representation.
    #[error("descriptor unit {unit} has invalid package source-closure hash `{value}`: {reason}")]
    InvalidPackageSourceClosureHash {
        /// Descriptor unit.
        unit: String,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cross-phase logical environment identity must be lowercase SHA-256.
    #[error("descriptor unit {unit} has invalid logical environment hash `{value}`: {reason}")]
    InvalidLogicalEnvironmentHash {
        /// Descriptor unit.
        unit: String,
        /// Rejected hash.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Two descriptor units cannot be selected by one final coordinate.
    #[error(
        "final unit coordinate {coordinate:?} ambiguously identifies descriptor units {first} and {second}"
    )]
    AmbiguousCoordinate {
        /// Ambiguous semantic coordinate.
        coordinate: FinalUnitCoordinate,
        /// First unit.
        first: String,
        /// Second unit.
        second: String,
    },
    /// JSON serialization failed.
    #[error("failed to serialize final unit map: {0}")]
    Serialize(serde_json::Error),
    /// A content-addressed path must have a parent.
    #[error("final unit-map path has no parent: `{0}`")]
    PathWithoutParent(PathBuf),
    /// Content-addressed directory creation failed.
    #[error("failed to create final unit-map directory `{path}`: {source}")]
    CreateDirectory {
        /// Directory.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Content-addressed file name must be valid UTF-8.
    #[error("invalid final unit-map filename: `{0}`")]
    InvalidFilename(PathBuf),
    /// Atomic write failed.
    #[error("failed to write final unit map `{path}`: {source}")]
    Write {
        /// Exact path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A same-hash path contained other bytes.
    #[error("content-addressed final unit map conflicts with `{0}`")]
    ConflictingContent(PathBuf),
}

/// One strictly parsed Cargo `compiler-artifact` message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CargoCompilerArtifact {
    /// Cargo package ID, exactly as Cargo emitted it.
    pub package_id: String,
    /// Manifest path reported by Cargo.
    pub manifest_path: PathBuf,
    /// Target identity reported by Cargo.
    pub target: CargoArtifactTarget,
    /// Profile object reported by Cargo.
    pub profile: Value,
    /// Enabled features, normalized for exact matching.
    pub features: BTreeSet<String>,
    /// Exact artifact paths reported by Cargo.
    pub filenames: BTreeSet<PathBuf>,
    /// Executable path, when this target has one.
    pub executable: Option<PathBuf>,
    /// Whether Cargo reused the artifact.
    pub fresh: bool,
}

/// Target portion of a Cargo `compiler-artifact` message.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CargoArtifactTarget {
    /// Rust target name.
    pub name: String,
    /// Cargo target kinds.
    pub kind: BTreeSet<String>,
    /// Rust crate types.
    pub crate_types: BTreeSet<String>,
    /// Target source path.
    pub src_path: PathBuf,
    /// Rust edition.
    pub edition: String,
}

#[derive(Debug, Deserialize)]
struct RawCargoArtifact {
    package_id: String,
    manifest_path: PathBuf,
    target: RawCargoTarget,
    profile: Value,
    features: Vec<String>,
    filenames: Vec<PathBuf>,
    executable: Option<PathBuf>,
    fresh: bool,
}

#[derive(Debug, Deserialize)]
struct RawCargoTarget {
    name: String,
    kind: Vec<String>,
    crate_types: Vec<String>,
    src_path: PathBuf,
    edition: String,
}

/// Strict Cargo JSON collector. It never searches a target directory.
#[derive(Debug, Default)]
pub struct CargoArtifactCollector {
    artifacts: Vec<CargoCompilerArtifact>,
    seen_artifacts: BTreeSet<Vec<u8>>,
    build_finished: Option<bool>,
    line: usize,
}

impl CargoArtifactCollector {
    /// Consume one complete line from `cargo --message-format=json...`.
    pub fn ingest_line(&mut self, line: &str) -> Result<(), CargoMessageError> {
        self.line += 1;
        if line.trim().is_empty() {
            return Err(CargoMessageError::EmptyLine { line: self.line });
        }
        if self.build_finished.is_some() {
            return Err(CargoMessageError::MessageAfterBuildFinished { line: self.line });
        }

        let value: Value =
            serde_json::from_str(line).map_err(|source| CargoMessageError::MalformedJson {
                line: self.line,
                source,
            })?;
        let reason = value
            .get("reason")
            .and_then(Value::as_str)
            .ok_or(CargoMessageError::MissingReason { line: self.line })?;

        match reason {
            "compiler-artifact" => {
                let raw: RawCargoArtifact = serde_json::from_value(value).map_err(|source| {
                    CargoMessageError::MalformedCompilerArtifact {
                        line: self.line,
                        source,
                    }
                })?;
                let unique_kinds = raw.target.kind.iter().collect::<BTreeSet<_>>();
                let unique_crate_types = raw.target.crate_types.iter().collect::<BTreeSet<_>>();
                let unique_features = raw.features.iter().collect::<BTreeSet<_>>();
                let unique_filenames = raw.filenames.iter().collect::<BTreeSet<_>>();
                if raw.package_id.is_empty()
                    || !raw.manifest_path.is_absolute()
                    || raw.target.name.is_empty()
                    || raw.target.kind.is_empty()
                    || raw.target.crate_types.is_empty()
                    || !raw.target.src_path.is_absolute()
                    || raw.target.edition.is_empty()
                    || !raw.profile.is_object()
                    || raw.filenames.is_empty()
                    || raw.filenames.iter().any(|path| !path.is_absolute())
                    || unique_kinds.len() != raw.target.kind.len()
                    || unique_crate_types.len() != raw.target.crate_types.len()
                    || unique_features.len() != raw.features.len()
                    || unique_filenames.len() != raw.filenames.len()
                {
                    return Err(CargoMessageError::IncompleteCompilerArtifact { line: self.line });
                }
                let artifact = CargoCompilerArtifact {
                    package_id: raw.package_id,
                    manifest_path: raw.manifest_path,
                    target: CargoArtifactTarget {
                        name: raw.target.name,
                        kind: raw.target.kind.into_iter().collect(),
                        crate_types: raw.target.crate_types.into_iter().collect(),
                        src_path: raw.target.src_path,
                        edition: raw.target.edition,
                    },
                    profile: raw.profile,
                    features: raw.features.into_iter().collect(),
                    filenames: raw.filenames.into_iter().collect(),
                    executable: raw.executable,
                    fresh: raw.fresh,
                };
                let identity = serde_json::to_vec(&artifact)
                    .expect("serializing an in-memory Cargo artifact cannot fail");
                if !self.seen_artifacts.insert(identity) {
                    return Err(CargoMessageError::DuplicateCompilerArtifact {
                        line: self.line,
                        package_id: artifact.package_id,
                        target: artifact.target.name,
                    });
                }
                self.artifacts.push(artifact);
            }
            "build-finished" => {
                let success = value
                    .get("success")
                    .and_then(Value::as_bool)
                    .ok_or(CargoMessageError::MalformedBuildFinished { line: self.line })?;
                self.build_finished = Some(success);
            }
            "compiler-message" => {
                validate_auxiliary_message(
                    &value,
                    self.line,
                    reason,
                    &[
                        ("package_id", JsonFieldKind::NonemptyString),
                        ("target", JsonFieldKind::Object),
                        ("message", JsonFieldKind::Object),
                    ],
                )?;
            }
            "build-script-executed" => {
                validate_auxiliary_message(
                    &value,
                    self.line,
                    reason,
                    &[
                        ("package_id", JsonFieldKind::NonemptyString),
                        ("linked_libs", JsonFieldKind::Array),
                        ("linked_paths", JsonFieldKind::Array),
                        ("cfgs", JsonFieldKind::Array),
                        ("env", JsonFieldKind::Array),
                        ("out_dir", JsonFieldKind::NonemptyString),
                    ],
                )?;
            }
            "future-incompat-report" => {
                validate_auxiliary_message(
                    &value,
                    self.line,
                    reason,
                    &[("future_incompat_report", JsonFieldKind::Object)],
                )?;
            }
            other => {
                return Err(CargoMessageError::UnknownReason {
                    line: self.line,
                    reason: other.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Consume a complete Cargo JSON stream.
    pub fn ingest_reader(&mut self, reader: impl BufRead) -> Result<(), CargoMessageError> {
        for line in reader.lines() {
            let line = line.map_err(|source| CargoMessageError::Read {
                line: self.line + 1,
                source,
            })?;
            self.ingest_line(&line)?;
        }
        Ok(())
    }

    /// Require a successful terminal message and return collected artifacts.
    pub fn finish(self) -> Result<Vec<CargoCompilerArtifact>, CargoMessageError> {
        match self.build_finished {
            Some(true) => Ok(self.artifacts),
            Some(false) => Err(CargoMessageError::CargoBuildFailed),
            None => Err(CargoMessageError::MissingBuildFinished),
        }
    }
}

/// One exact descriptor-phase Cargo invocation.
#[derive(Clone, Debug)]
pub struct DescriptorCargoRequest {
    /// Cargo executable.
    pub cargo: OsString,
    /// Absolute `cargo-proa`/`proa` executable used as `RUSTC_WRAPPER`.
    pub wrapper: PathBuf,
    /// Absolute Cargo working directory.
    pub working_directory: PathBuf,
    /// Fresh absolute staging directory reserved for this invocation. A
    /// cached recovery may substitute the exact marker-owned root named by an
    /// interrupted target journal and write through a fresh child attempt. It
    /// must be a strict descendant of `source_exclusion_directory` so wrapper
    /// outputs cannot enter package source identity.
    pub staging_directory: PathBuf,
    /// Exact absolute Cargo target directory reserved for descriptor builds.
    ///
    /// The runner owns `--target-dir`; callers must not repeat it in
    /// `cargo_args`. Recovery may remove this directory only after validating
    /// the runner-owned marker written here.
    pub target_directory: PathBuf,
    /// Exact generated subtree excluded from package source closure hashing.
    /// The descriptor target must be contained by this path. Linked final
    /// compilation receives the same boundary so the coordinate is phase
    /// invariant even for custom in-tree Cargo target directories.
    pub source_exclusion_directory: PathBuf,
    /// Cargo arguments, including the subcommand and exact target selection.
    ///
    /// The runner owns `--message-format` and rejects an author-supplied one.
    pub cargo_args: Vec<OsString>,
    /// Additional exact environment for both Cargo and build scripts.
    pub environment: BTreeMap<OsString, OsString>,
}

/// Successful descriptor Cargo output, before root-specific manifest linking.
///
/// This remains crate-private because its Cargo artifact paths point into a
/// mutable shared target. Public callers receive an opaque lease and must
/// consume it through the safe linking/reporting boundary below.
#[derive(Debug)]
pub(crate) struct DescriptorCargoOutput {
    /// Strictly parsed artifacts from Cargo's JSON protocol.
    pub(crate) cargo_artifacts: Vec<CargoCompilerArtifact>,
    /// Validated wrapper units and exact descriptor objects.
    pub(crate) units: Vec<ValidatedDescriptorUnit>,
    /// Exact immutable byte snapshots admitted while the cache lease is held.
    /// Manifest hashing and extraction consume these same bytes, never a later
    /// reread of the mutable path. Uncached callers may leave this map empty.
    pub(crate) objects: BTreeMap<PathBuf, DescriptorObjectSnapshot>,
    /// Input and link hashes checked against exact extern authority in this run.
    pub(crate) input_hashes: DescriptorInputHashes,
}

#[derive(Clone, Debug)]
pub(crate) struct DescriptorObjectSnapshot {
    // Retain the allocation returned by fs::read. Converting its Vec into an
    // Arc<[u8]> allocates again and copies the entire compiler artifact.
    pub(crate) bytes: std::sync::Arc<Vec<u8>>,
    pub(crate) descriptor_hash: [u8; 32],
    pub(crate) link_hash: [u8; 32],
}

impl DescriptorObjectSnapshot {
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let mut descriptor = Sha256::new();
        descriptor.update(DESCRIPTOR_OBJECT_HASH_DOMAIN);
        descriptor.update(&bytes);
        let mut link = Sha256::new();
        link.update(LINK_ARTIFACT_HASH_DOMAIN);
        link.update(&bytes);
        Ok(Self {
            bytes: std::sync::Arc::new(bytes),
            descriptor_hash: descriptor.finalize().into(),
            link_hash: link.finalize().into(),
        })
    }
}

/// Path-free summary of one descriptor Cargo artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorCargoArtifactReport {
    /// Cargo package ID, exactly as Cargo emitted it.
    pub package_id: String,
    /// Cargo target name.
    pub target_name: String,
    /// Cargo target-kind set.
    pub target_kinds: BTreeSet<String>,
    /// Whether Cargo reused this artifact.
    pub fresh: bool,
}

/// Path-free summary of one validated descriptor unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorCargoUnitReport {
    /// Strong descriptor Cargo-unit identity.
    pub unit: DescriptorCargoUnitId,
    /// Cargo package name recorded by the wrapper, when available.
    pub cargo_package_name: Option<String>,
    /// Rust crate name of the compiled unit. Distinguishes a package's lib
    /// unit from its `build_script_build` unit, which share one package name.
    pub crate_name: String,
    /// Exact descriptor object content hash.
    pub descriptor_object_hash: String,
}

/// Safe descriptor-run diagnostics that remain valid after its leases drop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorCargoReport {
    /// Path-free Cargo artifact summaries.
    pub cargo_artifacts: Vec<DescriptorCargoArtifactReport>,
    /// Path-free descriptor-unit summaries.
    pub units: Vec<DescriptorCargoUnitReport>,
}

impl DescriptorCargoReport {
    pub(crate) fn from_output(output: &DescriptorCargoOutput) -> Self {
        Self {
            cargo_artifacts: output
                .cargo_artifacts
                .iter()
                .map(|artifact| DescriptorCargoArtifactReport {
                    package_id: artifact.package_id.clone(),
                    target_name: artifact.target.name.clone(),
                    target_kinds: artifact.target.kind.clone(),
                    fresh: artifact.fresh,
                })
                .collect(),
            units: output
                .units
                .iter()
                .map(|unit| DescriptorCargoUnitReport {
                    unit: unit.unit,
                    cargo_package_name: unit.sidecar.cargo_package_name.clone(),
                    crate_name: unit.sidecar.crate_name.clone(),
                    descriptor_object_hash: unit.sidecar.descriptor_object_hash.clone(),
                })
                .collect(),
        }
    }
}

/// Exclusive lease for one canonical descriptor target, independent of which
/// cache directory or orchestration mode is using it.
pub(crate) struct DescriptorTargetLease {
    _file: File,
}

/// Opaque lease retaining exclusive authority over one descriptor target.
///
/// Consume it with `crate::rsjs::pipeline::link_descriptor_transport` or
/// [`finish_descriptor_run`].
#[must_use = "descriptor Cargo leases retain filesystem locks until consumed or dropped"]
pub struct DescriptorCargoLease {
    output: DescriptorCargoOutput,
    _target_lease: DescriptorTargetLease,
}

impl fmt::Debug for DescriptorCargoLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DescriptorCargoLease")
            .field("artifacts", &self.output.cargo_artifacts.len())
            .field("units", &self.output.units.len())
            .finish_non_exhaustive()
    }
}

/// Private half of the sealed public descriptor-run capability.
pub(crate) mod descriptor_run_sealed {
    use super::DescriptorCargoOutput;

    pub(crate) trait Sealed {
        fn output(&self) -> &DescriptorCargoOutput;

        fn into_output(self) -> DescriptorCargoOutput
        where
            Self: Sized;
    }
}

/// A descriptor Cargo run whose mutable target remains exclusively leased.
///
/// This trait is sealed. It exists so the safe consuming linker can accept
/// cached and uncached runs without exposing their raw path-bearing output.
#[allow(private_bounds)]
pub trait DescriptorCargoRun: descriptor_run_sealed::Sealed {}

impl descriptor_run_sealed::Sealed for DescriptorCargoLease {
    fn output(&self) -> &DescriptorCargoOutput {
        &self.output
    }

    fn into_output(self) -> DescriptorCargoOutput {
        self.output
    }
}

impl DescriptorCargoRun for DescriptorCargoLease {}

/// Borrow a path-free diagnostic report while retaining the run lease.
pub fn descriptor_run_report(run: &impl DescriptorCargoRun) -> DescriptorCargoReport {
    DescriptorCargoReport::from_output(descriptor_run_sealed::Sealed::output(run))
}

/// Consume a descriptor run without linking and return only path-free data.
pub fn finish_descriptor_run(run: impl DescriptorCargoRun) -> DescriptorCargoReport {
    let report = descriptor_run_report(&run);
    drop(descriptor_run_sealed::Sealed::into_output(run));
    report
}

/// Run descriptor compilation with the hidden wrapper and collect only
/// invocation-authoritative outputs.
pub fn run_descriptor_cargo(
    request: &DescriptorCargoRequest,
) -> Result<DescriptorCargoLease, DescriptorCargoRunError> {
    validate_descriptor_managed_paths(request)?;
    let target_lease = acquire_descriptor_target_lease(&request.target_directory)?;
    reject_cached_crash_journal(&request.target_directory)?;
    let output = run_descriptor_cargo_with_empty_policy(
        request,
        false,
        false,
        DescriptorStagingPreparation::Fresh,
    )?;
    Ok(DescriptorCargoLease {
        output,
        _target_lease: target_lease,
    })
}

/// Remove one successfully consumed invocation-local descriptor staging tree.
///
/// This is deliberately not part of `DescriptorCargoLease`'s drop path: failed
/// linking/finalization keeps its exact staging evidence. Callers may invoke
/// this only after all staging-backed descriptor units have been consumed.
/// The exact request path is revalidated and must still carry the marker that
/// the descriptor runner wrote before only that staging directory is removed.
pub(crate) fn remove_successful_descriptor_staging(
    request: &DescriptorCargoRequest,
) -> Result<(), DescriptorCargoRunError> {
    let managed = validate_descriptor_managed_paths(request)?;
    let staging = managed.staging;
    let owned = descriptor_staging_marker_is_valid(&staging).map_err(|source| {
        DescriptorCargoRunError::ReadCompletedStagingMarker {
            path: staging.clone(),
            source,
        }
    })?;
    if !owned {
        return Err(DescriptorCargoRunError::UnownedCompletedStaging(staging));
    }
    let metadata = fs::symlink_metadata(&staging).map_err(|source| {
        DescriptorCargoRunError::InspectCompletedStaging {
            path: staging.clone(),
            source,
        }
    })?;
    if !metadata.file_type().is_dir() {
        return Err(DescriptorCargoRunError::UnsafeCompletedStaging(staging));
    }
    fs::remove_dir_all(&staging).map_err(|source| DescriptorCargoRunError::RemoveCompletedStaging {
        path: staging,
        source,
    })
}

fn reject_cached_crash_journal(target_directory: &Path) -> Result<(), DescriptorCargoRunError> {
    let journal = descriptor_target_dirty_journal_path(target_directory)?;
    match fs::symlink_metadata(&journal) {
        Ok(metadata) if metadata.file_type().is_file() => Err(
            DescriptorCargoRunError::CachedCrashJournalRequiresRecovery(journal),
        ),
        Ok(_) => Err(DescriptorCargoRunError::UnsafeTargetJournal(journal)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DescriptorCargoRunError::ReadTargetJournal {
            path: journal,
            source,
        }),
    }
}

pub(crate) fn run_descriptor_cargo_allow_empty(
    request: &DescriptorCargoRequest,
) -> Result<DescriptorCargoOutput, DescriptorCargoRunError> {
    // This cache-only path consumes a unique staging directory written by the
    // wrapper child that just exited. The wrapper hashes each object before it
    // atomically publishes the sidecar; the cache then verifies the same hash
    // while materializing the object before exposing the merged output.
    run_descriptor_cargo_with_empty_policy(request, true, true, DescriptorStagingPreparation::Fresh)
}

/// Resume the exact marker-owned staging root recorded by an interrupted
/// cached descriptor transaction.
///
/// Cargo still runs against the persistent target and emits a new complete
/// artifact enumeration. The wrapper writes through a fresh child attempt so
/// recompiling the same unit cannot overwrite earlier provisional evidence.
/// Sidecars from every completed attempt remain candidates until the cache
/// revalidates them against that enumeration and the current inputs.
pub(crate) fn run_descriptor_cargo_allow_empty_resuming(
    request: &DescriptorCargoRequest,
) -> Result<DescriptorCargoOutput, DescriptorCargoRunError> {
    run_descriptor_cargo_with_empty_policy(
        request,
        true,
        false,
        DescriptorStagingPreparation::ResumeOwned,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescriptorStagingPreparation {
    Fresh,
    ResumeOwned,
}

fn run_descriptor_cargo_with_empty_policy(
    request: &DescriptorCargoRequest,
    allow_empty_units: bool,
    trust_atomic_wrapper_hashes: bool,
    staging_preparation: DescriptorStagingPreparation,
) -> Result<DescriptorCargoOutput, DescriptorCargoRunError> {
    if !request.wrapper.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeWrapper(
            request.wrapper.clone(),
        ));
    }
    if !request.working_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeWorkingDirectory(
            request.working_directory.clone(),
        ));
    }
    if !request.staging_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeStagingDirectory(
            request.staging_directory.clone(),
        ));
    }
    validate_descriptor_managed_paths(request)?;
    if request.cargo_args.is_empty() {
        return Err(DescriptorCargoRunError::MissingCargoArguments);
    }
    if env::var_os(COMPONENT_DESCRIPTOR_DIRS_ENV).is_some()
        || request
            .environment
            .contains_key(OsStr::new(COMPONENT_DESCRIPTOR_DIRS_ENV))
    {
        return Err(DescriptorCargoRunError::UnsupportedEnvironment(
            COMPONENT_DESCRIPTOR_DIRS_ENV,
        ));
    }
    if let Some(argument) = request.cargo_args.iter().find(|argument| {
        let argument = argument.to_string_lossy();
        argument.starts_with("--message-format")
            || argument == "--target-dir"
            || argument.starts_with("--target-dir=")
    }) {
        return Err(DescriptorCargoRunError::ReservedCargoArgument(
            argument.clone(),
        ));
    }
    if let Some(name) = request
        .environment
        .keys()
        .find(|name| name.to_str().is_some_and(wrapper_control_environment))
    {
        return Err(DescriptorCargoRunError::ReservedEnvironment(name.clone()));
    }
    if env::var_os("RUSTC_WORKSPACE_WRAPPER").is_some()
        || request
            .environment
            .contains_key(OsStr::new("RUSTC_WORKSPACE_WRAPPER"))
    {
        return Err(DescriptorCargoRunError::ExistingWorkspaceWrapper);
    }
    if env::var_os("RUSTC_WRAPPER").is_some()
        || request
            .environment
            .contains_key(OsStr::new("RUSTC_WRAPPER"))
    {
        return Err(DescriptorCargoRunError::ExistingRustcWrapper);
    }
    prepare_descriptor_target_directory(&request.target_directory)?;
    let staging_parent = request
        .staging_directory
        .parent()
        .ok_or_else(|| {
            DescriptorCargoRunError::StagingWithoutParent(request.staging_directory.clone())
        })?
        .to_path_buf();
    fs::create_dir_all(&staging_parent).map_err(|source| {
        DescriptorCargoRunError::CreateStagingParent {
            path: staging_parent,
            source,
        }
    })?;
    let wrapper_staging = prepare_descriptor_staging_directory(request, staging_preparation)?;

    let mut command = Command::new(&request.cargo);
    command
        .current_dir(&request.working_directory)
        .args(&request.cargo_args)
        .arg("--target-dir")
        .arg(&request.target_directory)
        .arg("--message-format=json-render-diagnostics")
        .envs(&request.environment);
    scrub_wrapper_control_environment(&mut command);
    command
        .env("RUSTC_WRAPPER", &request.wrapper)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env(WRAPPER_MODE_ENV, WRAPPER_MODE_DESCRIPTOR_V1)
        .env(DESCRIPTOR_STAGING_DIR_ENV, &wrapper_staging)
        .env(DESCRIPTOR_TARGET_DIR_ENV, &request.target_directory)
        .env(
            SOURCE_EXCLUSION_DIR_ENV,
            &request.source_exclusion_directory,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .map_err(|source| DescriptorCargoRunError::SpawnCargo {
            program: PathBuf::from(&request.cargo),
            source,
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or(DescriptorCargoRunError::MissingCargoStdout)?;
    let mut collector = CargoArtifactCollector::default();
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(DescriptorCargoRunError::ReadCargoStdout(source));
            }
        };
        forward_rendered_cargo_diagnostic(&line);
        if let Err(source) = collector.ingest_line(&line) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DescriptorCargoRunError::CargoProtocol(source));
        }
    }
    let status = child
        .wait()
        .map_err(DescriptorCargoRunError::WaitForCargo)?;
    if !status.success() {
        return Err(DescriptorCargoRunError::CargoFailed(status));
    }
    let cargo_artifacts = collector
        .finish()
        .map_err(DescriptorCargoRunError::CargoProtocol)?;
    let units = if staging_preparation == DescriptorStagingPreparation::ResumeOwned {
        load_descriptor_recovery_units(&request.staging_directory)?
    } else {
        let units_directory = request.staging_directory.join("units");
        if allow_empty_units && !units_directory.exists() {
            Vec::new()
        } else {
            let loaded = if trust_atomic_wrapper_hashes {
                load_descriptor_unit_sidecars_trusting_wrapper_hashes(&request.staging_directory)
            } else {
                load_descriptor_unit_sidecars(&request.staging_directory)
            };
            match loaded {
                Ok(units) => units,
                Err(DescriptorSidecarError::NoUnitSidecars(_)) if allow_empty_units => Vec::new(),
                Err(source) => return Err(DescriptorCargoRunError::Sidecars(source)),
            }
        }
    };
    Ok(DescriptorCargoOutput {
        cargo_artifacts,
        units,
        objects: BTreeMap::new(),
        input_hashes: BTreeMap::new(),
    })
}

fn prepare_descriptor_staging_directory(
    request: &DescriptorCargoRequest,
    preparation: DescriptorStagingPreparation,
) -> Result<PathBuf, DescriptorCargoRunError> {
    let metadata = match fs::symlink_metadata(&request.staging_directory) {
        Ok(metadata) => Some(metadata),
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(DescriptorCargoRunError::InspectCompletedStaging {
                path: request.staging_directory.clone(),
                source,
            });
        }
    };

    if let Some(metadata) = metadata {
        if preparation == DescriptorStagingPreparation::Fresh {
            return Err(DescriptorCargoRunError::StagingAlreadyExists(
                request.staging_directory.clone(),
            ));
        }
        if !metadata.file_type().is_dir() {
            return Err(DescriptorCargoRunError::UnsafeCompletedStaging(
                request.staging_directory.clone(),
            ));
        }
        let owned =
            descriptor_staging_marker_is_valid(&request.staging_directory).map_err(|source| {
                DescriptorCargoRunError::ReadCompletedStagingMarker {
                    path: request.staging_directory.clone(),
                    source,
                }
            })?;
        if !owned {
            return Err(DescriptorCargoRunError::UnownedCompletedStaging(
                request.staging_directory.clone(),
            ));
        }
        return create_descriptor_recovery_attempt(&request.staging_directory);
    }

    fs::create_dir(&request.staging_directory).map_err(|source| {
        DescriptorCargoRunError::CreateStaging {
            path: request.staging_directory.clone(),
            source,
        }
    })?;
    write_ownership_marker(
        &request.staging_directory,
        DESCRIPTOR_STAGING_MARKER_FILE,
        DESCRIPTOR_STAGING_MARKER_BYTES,
    )
    .map_err(|source| DescriptorCargoRunError::CreateStagingMarker {
        path: request.staging_directory.clone(),
        source,
    })?;
    match preparation {
        DescriptorStagingPreparation::Fresh => Ok(request.staging_directory.clone()),
        DescriptorStagingPreparation::ResumeOwned => {
            create_descriptor_recovery_attempt(&request.staging_directory)
        }
    }
}

fn create_descriptor_recovery_attempt(
    staging_directory: &Path,
) -> Result<PathBuf, DescriptorCargoRunError> {
    let attempts_directory = staging_directory.join(DESCRIPTOR_RECOVERY_ATTEMPTS_DIRECTORY);
    match fs::symlink_metadata(&attempts_directory) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(DescriptorCargoRunError::UnexpectedRecoveryStagingEntry(
                attempts_directory,
            ));
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&attempts_directory).map_err(|source| {
                DescriptorCargoRunError::RecoveryStagingIo {
                    operation: "create",
                    path: attempts_directory.clone(),
                    source,
                }
            })?;
        }
        Err(source) => {
            return Err(DescriptorCargoRunError::RecoveryStagingIo {
                operation: "inspect",
                path: attempts_directory,
                source,
            });
        }
    }
    // Validate every prior attempt before adding another writer location. A
    // repeated recovery may leave any number of empty or partially populated
    // attempts, but never an unrecognized entry or redirected path.
    descriptor_recovery_staging_directories(staging_directory)?;

    loop {
        let nonce = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
        let attempt = attempts_directory.join(format!(
            "{DESCRIPTOR_RECOVERY_ATTEMPT_PREFIX}{}-{nonce}",
            std::process::id()
        ));
        match fs::create_dir(&attempt) {
            Ok(()) => return Ok(attempt),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(DescriptorCargoRunError::RecoveryStagingIo {
                    operation: "create",
                    path: attempt,
                    source,
                });
            }
        }
    }
}

fn load_descriptor_recovery_units(
    staging_directory: &Path,
) -> Result<Vec<ValidatedDescriptorUnit>, DescriptorCargoRunError> {
    let mut units = Vec::new();
    for candidate_staging in descriptor_recovery_staging_directories(staging_directory)? {
        let units_directory = candidate_staging.join("units");
        match fs::symlink_metadata(&units_directory) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(DescriptorCargoRunError::UnexpectedRecoveryStagingEntry(
                    units_directory,
                ));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(DescriptorCargoRunError::RecoveryStagingIo {
                    operation: "inspect",
                    path: units_directory,
                    source,
                });
            }
        }
        match load_descriptor_unit_sidecars_for_recovery(&candidate_staging) {
            Ok(mut candidate_units) => units.append(&mut candidate_units),
            Err(DescriptorSidecarError::NoUnitSidecars(_)) => {}
            Err(source) => return Err(DescriptorCargoRunError::Sidecars(source)),
        }
    }
    Ok(units)
}

fn descriptor_recovery_staging_directories(
    staging_directory: &Path,
) -> Result<Vec<PathBuf>, DescriptorCargoRunError> {
    let attempts_directory = staging_directory.join(DESCRIPTOR_RECOVERY_ATTEMPTS_DIRECTORY);
    let metadata = match fs::symlink_metadata(&attempts_directory) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(vec![staging_directory.to_path_buf()]);
        }
        Err(source) => {
            return Err(DescriptorCargoRunError::RecoveryStagingIo {
                operation: "inspect",
                path: attempts_directory,
                source,
            });
        }
    };
    if !metadata.file_type().is_dir() {
        return Err(DescriptorCargoRunError::UnexpectedRecoveryStagingEntry(
            attempts_directory,
        ));
    }

    let entries = fs::read_dir(&attempts_directory).map_err(|source| {
        DescriptorCargoRunError::RecoveryStagingIo {
            operation: "enumerate",
            path: attempts_directory.clone(),
            source,
        }
    })?;
    let mut directories = vec![staging_directory.to_path_buf()];
    for entry in entries {
        let entry = entry.map_err(|source| DescriptorCargoRunError::RecoveryStagingIo {
            operation: "enumerate",
            path: attempts_directory.clone(),
            source,
        })?;
        let path = entry.path();
        let file_type =
            entry
                .file_type()
                .map_err(|source| DescriptorCargoRunError::RecoveryStagingIo {
                    operation: "inspect",
                    path: path.clone(),
                    source,
                })?;
        if !file_type.is_dir() || !descriptor_recovery_attempt_name(entry.file_name().as_os_str()) {
            return Err(DescriptorCargoRunError::UnexpectedRecoveryStagingEntry(
                path,
            ));
        }
        directories.push(path);
    }
    directories[1..].sort();
    Ok(directories)
}

fn descriptor_recovery_attempt_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(coordinate) = name.strip_prefix(DESCRIPTOR_RECOVERY_ATTEMPT_PREFIX) else {
        return false;
    };
    let Some((process, nonce)) = coordinate.split_once('-') else {
        return false;
    };
    !process.is_empty()
        && process.bytes().all(|byte| byte.is_ascii_digit())
        && !nonce.is_empty()
        && nonce.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedDescriptorManagedPaths {
    pub(crate) working: PathBuf,
    pub(crate) staging: PathBuf,
    pub(crate) target: PathBuf,
    pub(crate) source_exclusion: PathBuf,
}

pub(crate) fn validate_descriptor_managed_paths(
    request: &DescriptorCargoRequest,
) -> Result<ValidatedDescriptorManagedPaths, DescriptorCargoRunError> {
    if !request.working_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeWorkingDirectory(
            request.working_directory.clone(),
        ));
    }
    if !request.staging_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeStagingDirectory(
            request.staging_directory.clone(),
        ));
    }
    if !request.target_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeTargetDirectory(
            request.target_directory.clone(),
        ));
    }
    if !request.source_exclusion_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeSourceExclusionDirectory(
            request.source_exclusion_directory.clone(),
        ));
    }
    for path in [
        &request.working_directory,
        &request.staging_directory,
        &request.target_directory,
        &request.source_exclusion_directory,
    ] {
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        }) {
            return Err(DescriptorCargoRunError::AmbiguousManagedDirectory(
                path.clone(),
            ));
        }
    }
    let normalized_working = normalize_descriptor_managed_path(&request.working_directory)?;
    let normalized_target = normalize_descriptor_managed_path(&request.target_directory)?;
    let normalized_staging = normalize_descriptor_managed_path(&request.staging_directory)?;
    let normalized_exclusion =
        normalize_descriptor_managed_path(&request.source_exclusion_directory)?;
    if normalized_target == normalized_exclusion
        || !normalized_target.starts_with(&normalized_exclusion)
    {
        return Err(DescriptorCargoRunError::TargetOutsideSourceExclusion {
            target: normalized_target,
            exclusion: normalized_exclusion,
        });
    }
    if normalized_staging == normalized_exclusion
        || !normalized_staging.starts_with(&normalized_exclusion)
    {
        return Err(DescriptorCargoRunError::StagingOutsideSourceExclusion {
            staging: normalized_staging,
            exclusion: normalized_exclusion,
        });
    }
    validate_plain_managed_descendant(
        "target",
        &request.source_exclusion_directory,
        &request.target_directory,
    )?;
    validate_plain_managed_descendant(
        "staging",
        &request.source_exclusion_directory,
        &request.staging_directory,
    )?;
    if managed_paths_overlap(&normalized_target, &normalized_staging) {
        return Err(
            DescriptorCargoRunError::OverlappingTargetAndStagingDirectories {
                target: normalized_target.clone(),
                staging: normalized_staging.clone(),
            },
        );
    }
    if normalized_exclusion == normalized_working
        || normalized_working.starts_with(&normalized_exclusion)
    {
        return Err(DescriptorCargoRunError::UnsafeSourceExclusion {
            exclusion: normalized_exclusion,
            working: normalized_working,
        });
    }
    Ok(ValidatedDescriptorManagedPaths {
        working: normalized_working,
        staging: normalized_staging,
        target: normalized_target,
        source_exclusion: normalized_exclusion,
    })
}

pub(crate) fn acquire_descriptor_target_lease(
    target_directory: &Path,
) -> Result<DescriptorTargetLease, DescriptorCargoRunError> {
    let lock_path = descriptor_target_state_path(target_directory, "lock")?;
    let lock_directory = lock_path
        .parent()
        .expect("descriptor target state paths always have a lock directory");
    create_plain_lock_directory(lock_directory)?;
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(DescriptorCargoRunError::UnsafeTargetLock(lock_path));
        }
        Ok(_) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DescriptorCargoRunError::OpenTargetLock {
                path: lock_path,
                source,
            });
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| DescriptorCargoRunError::OpenTargetLock {
            path: lock_path.clone(),
            source,
        })?;
    file.lock()
        .map_err(|source| DescriptorCargoRunError::AcquireTargetLock {
            path: lock_path,
            source,
        })?;
    Ok(DescriptorTargetLease { _file: file })
}

/// Crash journal shared by every cache namespace that operates on one exact
/// descriptor target. The caller must hold [`DescriptorTargetLease`] while
/// reading, replacing, or removing this path.
pub(crate) fn descriptor_target_dirty_journal_path(
    target_directory: &Path,
) -> Result<PathBuf, DescriptorCargoRunError> {
    descriptor_target_state_path(target_directory, "dirty.json")
}

fn descriptor_target_state_path(
    target_directory: &Path,
    suffix: &str,
) -> Result<PathBuf, DescriptorCargoRunError> {
    if !target_directory.is_absolute() {
        return Err(DescriptorCargoRunError::RelativeTargetDirectory(
            target_directory.to_path_buf(),
        ));
    }
    let target = normalize_descriptor_managed_path(target_directory)?;
    let parent = target
        .parent()
        .ok_or_else(|| DescriptorCargoRunError::TargetLockWithoutParent(target.clone()))?;
    let path_bytes = target
        .to_str()
        .ok_or_else(|| DescriptorCargoRunError::NonUtf8TargetLock(target.clone()))?
        .as_bytes();
    let mut hash = Sha256::new();
    hash.update(b"proa.rsjs.descriptor-target-lock.v1");
    hash.update((path_bytes.len() as u64).to_be_bytes());
    hash.update(path_bytes);
    let identity = hex_digest(&hash.finalize().into());
    Ok(parent
        .join(".proa-rsjs-target-locks")
        .join(format!("{identity}.{suffix}")))
}

pub(crate) fn normalize_descriptor_managed_path(
    path: &Path,
) -> Result<PathBuf, DescriptorCargoRunError> {
    if contains_lexical_dot_component(path) {
        return Err(DescriptorCargoRunError::AmbiguousManagedDirectory(
            path.to_path_buf(),
        ));
    }
    normalize_managed_path_identity(path).map_err(|source| {
        DescriptorCargoRunError::NormalizeManagedDirectory {
            path: path.to_path_buf(),
            source,
        }
    })
}

pub(crate) fn managed_paths_overlap(first: &Path, second: &Path) -> bool {
    first == second || first.starts_with(second) || second.starts_with(first)
}

fn validate_plain_managed_descendant(
    kind: &'static str,
    anchor: &Path,
    descendant: &Path,
) -> Result<(), DescriptorCargoRunError> {
    let relative = descendant.strip_prefix(anchor).map_err(|_| {
        DescriptorCargoRunError::ManagedDirectoryNotLexicallyContained {
            kind,
            path: descendant.to_path_buf(),
            anchor: anchor.to_path_buf(),
        }
    })?;
    if relative.as_os_str().is_empty() {
        return Err(
            DescriptorCargoRunError::ManagedDirectoryNotLexicallyContained {
                kind,
                path: descendant.to_path_buf(),
                anchor: anchor.to_path_buf(),
            },
        );
    }
    let mut current = anchor.to_path_buf();
    let mut reached_missing_component = false;
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(DescriptorCargoRunError::AmbiguousManagedDirectory(
                descendant.to_path_buf(),
            ));
        };
        current.push(component);
        if reached_missing_component {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(DescriptorCargoRunError::UnsafeManagedDirectoryComponent {
                    kind,
                    path: current,
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                reached_missing_component = true;
            }
            Err(source) => {
                return Err(DescriptorCargoRunError::InspectManagedDirectory {
                    kind,
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn normalize_managed_path_identity(path: &Path) -> io::Result<PathBuf> {
    use std::path::Component;

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "managed path is not absolute",
        ));
    }
    if contains_lexical_dot_component(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "managed path contains `.` or `..`",
        ));
    }
    let mut lexical = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                lexical.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !lexical.pop() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "managed path escapes its filesystem root",
                    ));
                }
            }
        }
    }
    let mut ancestor = lexical.clone();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(_) => break,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let name = ancestor.file_name().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "managed path has no existing ancestor",
                    )
                })?;
                suffix.push(name.to_os_string());
                ancestor.pop();
            }
            Err(source) => return Err(source),
        }
    }
    let mut normalized = fs::canonicalize(&ancestor)?;
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

#[cfg(unix)]
fn contains_lexical_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;

    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| matches!(component, b"." | b".."))
}

#[cfg(windows)]
fn contains_lexical_dot_component(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt as _;

    let mut component = Vec::new();
    for unit in path.as_os_str().encode_wide().chain([b'/' as u16]) {
        if unit == b'/' as u16 || unit == b'\\' as u16 {
            if component.as_slice() == [b'.' as u16]
                || component.as_slice() == [b'.' as u16, b'.' as u16]
            {
                return true;
            }
            component.clear();
        } else {
            component.push(unit);
        }
    }
    false
}

fn create_plain_lock_directory(path: &Path) -> Result<(), DescriptorCargoRunError> {
    fs::create_dir_all(path).map_err(|source| {
        DescriptorCargoRunError::CreateTargetLockDirectory {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        DescriptorCargoRunError::CreateTargetLockDirectory {
            path: path.to_path_buf(),
            source,
        }
    })?;
    if !metadata.file_type().is_dir() {
        return Err(DescriptorCargoRunError::UnsafeTargetLock(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

pub(crate) fn descriptor_target_marker_is_valid(path: &Path) -> io::Result<bool> {
    ownership_marker_is_valid(
        path,
        DESCRIPTOR_TARGET_MARKER_FILE,
        DESCRIPTOR_TARGET_MARKER_BYTES,
    )
}

pub(crate) fn descriptor_staging_marker_is_valid(path: &Path) -> io::Result<bool> {
    ownership_marker_is_valid(
        path,
        DESCRIPTOR_STAGING_MARKER_FILE,
        DESCRIPTOR_STAGING_MARKER_BYTES,
    )
}

fn ownership_marker_is_valid(path: &Path, name: &str, bytes: &[u8]) -> io::Result<bool> {
    let marker = path.join(name);
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(source),
    };
    if !metadata.file_type().is_file() {
        return Ok(false);
    }
    Ok(fs::read(marker)? == bytes)
}

pub(crate) fn prepare_descriptor_target_directory(
    path: &Path,
) -> Result<(), DescriptorCargoRunError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(DescriptorCargoRunError::DescriptorTargetNotDirectory(
                path.to_path_buf(),
            ));
        }
        Ok(_) => {
            if descriptor_target_marker_is_valid(path).map_err(|source| {
                DescriptorCargoRunError::ReadDescriptorTargetMarker {
                    path: path.to_path_buf(),
                    source,
                }
            })? {
                return Ok(());
            }
            let mut entries = fs::read_dir(path).map_err(|source| {
                DescriptorCargoRunError::ReadDescriptorTarget {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            if entries.next().is_some() {
                return Err(DescriptorCargoRunError::UnownedDescriptorTarget(
                    path.to_path_buf(),
                ));
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|source| {
                DescriptorCargoRunError::CreateDescriptorTarget {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        }
        Err(source) => {
            return Err(DescriptorCargoRunError::ReadDescriptorTarget {
                path: path.to_path_buf(),
                source,
            });
        }
    }

    write_ownership_marker(
        path,
        DESCRIPTOR_TARGET_MARKER_FILE,
        DESCRIPTOR_TARGET_MARKER_BYTES,
    )
    .map_err(
        |source| DescriptorCargoRunError::CreateDescriptorTargetMarker {
            path: path.join(DESCRIPTOR_TARGET_MARKER_FILE),
            source,
        },
    )
}

fn write_ownership_marker(path: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let marker = path.join(name);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker)?;
    file.write_all(bytes).and_then(|()| file.sync_all())?;
    sync_created_directory(path)
}

#[cfg(unix)]
fn sync_created_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_created_directory(_path: &Path) -> io::Result<()> {
    Ok(())
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

/// Descriptor Cargo process/orchestration failure.
#[derive(Debug, Error)]
pub enum DescriptorCargoRunError {
    /// Wrapper path is not invocation-stable.
    #[error("descriptor wrapper path must be absolute: `{0}`")]
    RelativeWrapper(PathBuf),
    /// Cargo working directory must be explicit.
    #[error("descriptor Cargo working directory must be absolute: `{0}`")]
    RelativeWorkingDirectory(PathBuf),
    /// Staging path must be explicit.
    #[error("descriptor staging directory must be absolute: `{0}`")]
    RelativeStagingDirectory(PathBuf),
    /// Descriptor target is an exact deletion/recovery boundary.
    #[error("descriptor target directory must be absolute: `{0}`")]
    RelativeTargetDirectory(PathBuf),
    /// Generated source exclusion must be explicit and phase invariant.
    #[error("descriptor source exclusion directory must be absolute: `{0}`")]
    RelativeSourceExclusionDirectory(PathBuf),
    /// Exact descriptor output must be contained by the generated subtree.
    #[error("descriptor target `{target}` must be inside source exclusion `{exclusion}`")]
    TargetOutsideSourceExclusion {
        /// Exact target.
        target: PathBuf,
        /// Exact excluded generated root.
        exclusion: PathBuf,
    },
    /// Invocation staging is generated state too; allowing it inside a
    /// package's hashed source tree makes later unit identities depend on
    /// compile order and stale sidecars.
    #[error("descriptor staging `{staging}` must be inside source exclusion `{exclusion}`")]
    StagingOutsideSourceExclusion {
        /// Fresh invocation-local staging directory.
        staging: PathBuf,
        /// Exact excluded generated root.
        exclusion: PathBuf,
    },
    /// Managed deletion/identity boundaries must have one unambiguous lexical
    /// spelling before filesystem canonicalization.
    #[error("descriptor managed directory contains `.` or `..`: `{0}`")]
    AmbiguousManagedDirectory(PathBuf),
    /// Managed outputs must be lexical descendants of the exact generated
    /// root as well as canonical descendants of it.
    #[error("descriptor {kind} `{path}` is not lexically inside generated root `{anchor}`")]
    ManagedDirectoryNotLexicallyContained {
        /// Managed output role.
        kind: &'static str,
        /// Rejected output root.
        path: PathBuf,
        /// Exact generated trust anchor.
        anchor: PathBuf,
    },
    /// Existing descendants may not redirect generated writes through a
    /// symlink or special file.
    #[error("unsafe existing descriptor {kind} path component: `{path}`")]
    UnsafeManagedDirectoryComponent {
        /// Managed output role.
        kind: &'static str,
        /// Unsafe existing component.
        path: PathBuf,
    },
    /// Existing managed descendants could not be inspected safely.
    #[error("failed to inspect descriptor {kind} path component `{path}`: {source}")]
    InspectManagedDirectory {
        /// Managed output role.
        kind: &'static str,
        /// Component being inspected.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Target and staging are independently deleted/replaced and therefore
    /// cannot contain one another.
    #[error("descriptor target `{target}` and staging `{staging}` overlap")]
    OverlappingTargetAndStagingDirectories {
        /// Persistent Cargo target.
        target: PathBuf,
        /// Invocation-local staging directory.
        staging: PathBuf,
    },
    /// Excluding the Cargo working directory (or one of its ancestors) would
    /// erase actual package inputs from source-closure identity.
    #[error(
        "descriptor source exclusion `{exclusion}` would contain working directory `{working}`"
    )]
    UnsafeSourceExclusion {
        /// Exact normalized generated root.
        exclusion: PathBuf,
        /// Exact normalized Cargo working directory.
        working: PathBuf,
    },
    /// A normalized target must retain a filesystem parent for its sibling
    /// lock namespace.
    #[error("descriptor target has no parent for its lock: `{0}`")]
    TargetLockWithoutParent(PathBuf),
    /// The lock identity must be portable canonical UTF-8 rather than a lossy
    /// rendering of an OS path.
    #[error("descriptor target lock path is not UTF-8: `{0}`")]
    NonUtf8TargetLock(PathBuf),
    /// Canonical managed-directory identity could not be resolved safely.
    #[error("failed to normalize descriptor managed directory `{path}`: {source}")]
    NormalizeManagedDirectory {
        /// Requested managed path.
        path: PathBuf,
        /// Path normalization failure.
        #[source]
        source: io::Error,
    },
    /// The sibling target-lock directory could not be created or inspected.
    #[error("failed to create descriptor target lock directory `{path}`: {source}")]
    CreateTargetLockDirectory {
        /// Lock directory.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Symlinks and non-files are never accepted as target-lock entries.
    #[error("unsafe descriptor target lock entry: `{0}`")]
    UnsafeTargetLock(PathBuf),
    /// The target lock could not be opened.
    #[error("failed to open descriptor target lock `{path}`: {source}")]
    OpenTargetLock {
        /// Exact lock file.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Another descriptor invocation's target lease could not be acquired.
    #[error("failed to acquire descriptor target lock `{path}`: {source}")]
    AcquireTargetLock {
        /// Exact lock file.
        path: PathBuf,
        /// Lock failure.
        #[source]
        source: io::Error,
    },
    /// Only the cached runner has enough authority to reconcile a cached
    /// crash journal with immutable generations and pointer commits.
    #[error(
        "descriptor target has an interrupted cached generation journal `{0}`; recover it with the cached runner or use a fresh descriptor target"
    )]
    CachedCrashJournalRequiresRecovery(PathBuf),
    /// A target crash journal may not be redirected or replaced by a special
    /// filesystem entry.
    #[error("unsafe descriptor target crash journal: `{0}`")]
    UnsafeTargetJournal(PathBuf),
    /// The target crash journal could not be inspected while holding the
    /// canonical target lease.
    #[error("failed to inspect descriptor target crash journal `{path}`: {source}")]
    ReadTargetJournal {
        /// Exact journal path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// No Cargo subcommand/selection was supplied.
    #[error("descriptor Cargo invocation has no arguments")]
    MissingCargoArguments,
    /// The runner owns Cargo's JSON protocol.
    #[error("descriptor Cargo argument `{0:?}` is reserved; cargo-proa supplies message format")]
    ReservedCargoArgument(OsString),
    /// Wrapper-control variables belong exclusively to cargo-proa.
    #[error("descriptor Cargo environment `{0:?}` is reserved")]
    ReservedEnvironment(OsString),
    /// Proc-macro filesystem fallbacks are not part of exact build identity.
    #[error("descriptor Cargo environment `{0}` is unsupported by the exact linked pipeline")]
    UnsupportedEnvironment(&'static str),
    /// Existing nonempty directories cannot be silently claimed for recovery.
    #[error("descriptor target directory is not owned by cargo-proa: `{0}`")]
    UnownedDescriptorTarget(PathBuf),
    /// The target path must be a directory.
    #[error("descriptor target path is not a directory: `{0}`")]
    DescriptorTargetNotDirectory(PathBuf),
    /// Descriptor target creation failed.
    #[error("failed to create descriptor target directory `{path}`: {source}")]
    CreateDescriptorTarget {
        /// Exact target.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Existing descriptor target state could not be inspected.
    #[error("failed to inspect descriptor target directory `{path}`: {source}")]
    ReadDescriptorTarget {
        /// Exact target.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Target ownership marker could not be read.
    #[error("failed to read descriptor target ownership marker in `{path}`: {source}")]
    ReadDescriptorTargetMarker {
        /// Exact target.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Target ownership marker could not be durably created.
    #[error("failed to create descriptor target ownership marker `{path}`: {source}")]
    CreateDescriptorTargetMarker {
        /// Exact marker.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Existing wrappers are not represented by the current identity model.
    #[error("RUSTC_WORKSPACE_WRAPPER is set; unset both Cargo rustc wrapper variables before invoking cargo-proa")]
    ExistingWorkspaceWrapper,
    /// An inherited wrapper is outside the descriptor/final cache identity.
    #[error(
        "RUSTC_WRAPPER is set; unset it before invoking cargo-proa so descriptor cache identity cannot bypass wrapper semantics"
    )]
    ExistingRustcWrapper,
    /// Ordinary invocations cannot reuse staging; cached crash recovery uses a
    /// separate marker-validated path that revalidates every retained record.
    #[error("descriptor staging directory already exists: `{0}`")]
    StagingAlreadyExists(PathBuf),
    /// Staging root has no parent.
    #[error("descriptor staging directory has no parent: `{0}`")]
    StagingWithoutParent(PathBuf),
    /// Staging parent creation failed.
    #[error("failed to create descriptor staging parent `{path}`: {source}")]
    CreateStagingParent {
        /// Parent path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Fresh staging creation failed.
    #[error("failed to create fresh descriptor staging directory `{path}`: {source}")]
    CreateStaging {
        /// Fresh path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Fresh staging ownership marker could not be durably created.
    #[error("failed to create descriptor staging ownership marker in `{path}`: {source}")]
    CreateStagingMarker {
        /// Fresh staging root.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A recovery attempt directory could not be created, inspected, or
    /// enumerated beneath the marker-owned staging root.
    #[error("failed to {operation} descriptor recovery staging path `{path}`: {source}")]
    RecoveryStagingIo {
        /// Filesystem operation that failed.
        operation: &'static str,
        /// Exact recovery path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Recovery attempts accept only strict runner-named plain directories.
    #[error("unexpected descriptor recovery staging entry: `{0}`")]
    UnexpectedRecoveryStagingEntry(PathBuf),
    /// Successful cleanup requires the descriptor runner's exact ownership
    /// marker to remain readable.
    #[error("failed to read completed descriptor staging ownership marker in `{path}`: {source}")]
    ReadCompletedStagingMarker {
        /// Exact invocation-local staging root.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A successful linked run cannot delete staging it cannot prove it owns.
    #[error("refusing to remove unowned completed descriptor staging directory `{0}`")]
    UnownedCompletedStaging(PathBuf),
    /// The marker-bearing staging root must still be a plain directory.
    #[error("refusing to remove non-directory completed descriptor staging path `{0}`")]
    UnsafeCompletedStaging(PathBuf),
    /// The completed staging root could not be inspected after marker
    /// validation.
    #[error("failed to inspect completed descriptor staging directory `{path}`: {source}")]
    InspectCompletedStaging {
        /// Exact invocation-local staging root.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Exact marker-owned staging cleanup failed.
    #[error("failed to remove completed descriptor staging directory `{path}`: {source}")]
    RemoveCompletedStaging {
        /// Exact invocation-local staging root.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Cargo could not be spawned.
    #[error("failed to run Cargo `{program}`: {source}")]
    SpawnCargo {
        /// Program path.
        program: PathBuf,
        /// Process failure.
        #[source]
        source: io::Error,
    },
    /// Piped Cargo stdout was unexpectedly unavailable.
    #[error("descriptor Cargo process has no piped stdout")]
    MissingCargoStdout,
    /// Cargo JSON stream could not be read.
    #[error("failed to read descriptor Cargo stdout: {0}")]
    ReadCargoStdout(io::Error),
    /// Cargo process wait failed.
    #[error("failed to wait for descriptor Cargo: {0}")]
    WaitForCargo(io::Error),
    /// Cargo exited unsuccessfully.
    #[error("descriptor Cargo compilation failed with {0}")]
    CargoFailed(ExitStatus),
    /// Cargo's JSON stream was malformed or incomplete.
    #[error(transparent)]
    CargoProtocol(CargoMessageError),
    /// Wrapper outputs did not validate.
    #[error(transparent)]
    Sidecars(DescriptorSidecarError),
}

#[derive(Clone, Copy)]
enum JsonFieldKind {
    NonemptyString,
    Array,
    Object,
}

fn validate_auxiliary_message(
    value: &Value,
    line: usize,
    reason: &str,
    required: &[(&str, JsonFieldKind)],
) -> Result<(), CargoMessageError> {
    let valid = required.iter().all(|(field, kind)| {
        value.get(*field).is_some_and(|value| match kind {
            JsonFieldKind::NonemptyString => value.as_str().is_some_and(|value| !value.is_empty()),
            JsonFieldKind::Array => value.is_array(),
            JsonFieldKind::Object => value.is_object(),
        })
    });
    if valid {
        Ok(())
    } else {
        Err(CargoMessageError::MalformedAuxiliaryMessage {
            line,
            reason: reason.to_string(),
        })
    }
}

/// Exact caller-provided selector for one wrapper unit.
#[derive(Clone, Debug, PartialEq)]
pub struct CargoArtifactExpectation {
    /// Descriptor unit to associate with the Cargo artifact.
    pub unit: DescriptorCargoUnitId,
    /// Exact Cargo package ID.
    pub package_id: String,
    /// Exact target name.
    pub target_name: String,
    /// Exact Cargo target-kind set.
    pub target_kinds: BTreeSet<String>,
    /// Exact crate-type set.
    pub crate_types: BTreeSet<String>,
    /// Exact enabled-feature set.
    pub features: BTreeSet<String>,
    /// Exact Cargo profile JSON.
    pub profile: Value,
    /// At least one exact Cargo-enumerated path required for the match.
    pub required_filenames: BTreeSet<PathBuf>,
}

/// Match each descriptor unit to exactly one Cargo-enumerated artifact.
pub fn match_exact_cargo_artifacts(
    artifacts: &[CargoCompilerArtifact],
    expectations: &[CargoArtifactExpectation],
) -> Result<BTreeMap<DescriptorCargoUnitId, CargoCompilerArtifact>, ArtifactMatchError> {
    let mut matched = BTreeMap::new();
    let mut claimed_messages = BTreeSet::new();

    for expectation in expectations {
        if expectation.required_filenames.is_empty() {
            return Err(ArtifactMatchError::NoRequiredFilenames(expectation.unit));
        }
        if matched.contains_key(&expectation.unit) {
            return Err(ArtifactMatchError::DuplicateUnitExpectation(
                expectation.unit,
            ));
        }

        let candidates = artifacts
            .iter()
            .enumerate()
            .filter(|(_, artifact)| {
                artifact.package_id == expectation.package_id
                    && artifact.target.name == expectation.target_name
                    && artifact.target.kind == expectation.target_kinds
                    && artifact.target.crate_types == expectation.crate_types
                    && artifact.features == expectation.features
                    && artifact.profile == expectation.profile
                    && expectation
                        .required_filenames
                        .is_subset(&artifact.filenames)
            })
            .collect::<Vec<_>>();

        let (message_index, artifact) = match candidates.as_slice() {
            [] => return Err(ArtifactMatchError::MissingArtifact(expectation.unit)),
            [(message_index, artifact)] => (*message_index, (*artifact).clone()),
            _ => {
                return Err(ArtifactMatchError::AmbiguousArtifact {
                    unit: expectation.unit,
                    matches: candidates.len(),
                });
            }
        };
        if !claimed_messages.insert(message_index) {
            return Err(ArtifactMatchError::ArtifactClaimedByMultipleUnits {
                artifact: artifact
                    .filenames
                    .iter()
                    .next()
                    .cloned()
                    .expect("collector rejects artifacts without filenames"),
            });
        }
        matched.insert(expectation.unit, artifact);
    }

    Ok(matched)
}

/// Join wrapper units to Cargo messages from the same invocation using exact
/// manifest/source identity plus the realized target, crate-type, and feature
/// sets.
///
/// Host paths participate only in this freshness join. They are excluded from
/// the canonical Cargo-unit and link-plan identities.
pub fn join_descriptor_units_to_cargo_artifacts(
    units: &[ValidatedDescriptorUnit],
    artifacts: &[CargoCompilerArtifact],
) -> Result<BTreeMap<DescriptorCargoUnitId, CargoCompilerArtifact>, UnitArtifactJoinError> {
    let mut joined = BTreeMap::new();
    let mut claimed_artifacts = BTreeSet::new();
    for unit in units {
        let manifest_path = unit
            .sidecar
            .cargo_manifest_path
            .as_ref()
            .ok_or(UnitArtifactJoinError::MissingCargoManifest(unit.unit))?;
        if !manifest_path.is_absolute() {
            return Err(UnitArtifactJoinError::RelativeCargoManifest {
                unit: unit.unit,
                path: manifest_path.clone(),
            });
        }
        let crate_types = unit
            .sidecar
            .crate_types
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if crate_types.len() != unit.sidecar.crate_types.len() {
            return Err(UnitArtifactJoinError::DuplicateCrateType(unit.unit));
        }
        let features = unit
            .sidecar
            .features
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if features.len() != unit.sidecar.features.len() {
            return Err(UnitArtifactJoinError::DuplicateFeature(unit.unit));
        }
        let cargo_extra_filename = unit.sidecar.cargo_extra_filename.as_deref();
        if let Some(cargo_extra_filename) = cargo_extra_filename {
            validate_cargo_extra_filename(cargo_extra_filename).map_err(|reason| {
                UnitArtifactJoinError::InvalidCargoExtraFilename {
                    unit: unit.unit,
                    value: cargo_extra_filename.to_string(),
                    reason,
                }
            })?;
        }

        let mut candidates = Vec::new();
        for (index, artifact) in artifacts.iter().enumerate() {
            if artifact.manifest_path != *manifest_path
                || normalize_cargo_crate_name(&artifact.target.name) != unit.sidecar.crate_name
                || artifact.target.crate_types != crate_types
                || artifact.features != features
            {
                continue;
            }
            let source_hash =
                hash_file(&artifact.target.src_path, FILE_HASH_DOMAIN).map_err(|source| {
                    UnitArtifactJoinError::HashCargoSource {
                        path: artifact.target.src_path.clone(),
                        source,
                    }
                })?;
            let output_identity_matches = match cargo_extra_filename {
                Some(suffix) => {
                    artifact_has_cargo_extra_filename(artifact, &unit.sidecar.crate_name, suffix)
                        || (unit.sidecar.cargo_primary_package
                            && crate_types.contains("bin")
                            && artifact.executable.is_some()
                            && artifact_has_unhashed_primary_output(
                                artifact,
                                &unit.sidecar.crate_name,
                            ))
                }
                // Cargo can also emit canonical unhashed outputs for a
                // dependency that declares `cdylib` alongside `rlib`. Match
                // the exact compiler-artifact record instead of assuming only
                // the primary package can lack `-C extra-filename`.
                None => artifact_has_unhashed_primary_output(artifact, &unit.sidecar.crate_name),
            };
            if hex_digest(&source_hash) == unit.sidecar.source_hash && output_identity_matches {
                candidates.push((index, artifact));
            }
        }

        let (index, artifact) = match candidates.as_slice() {
            [] => return Err(UnitArtifactJoinError::MissingArtifact(unit.unit)),
            [(index, artifact)] => (*index, (*artifact).clone()),
            _ => {
                return Err(UnitArtifactJoinError::AmbiguousArtifact {
                    unit: unit.unit,
                    matches: candidates.len(),
                });
            }
        };
        if !claimed_artifacts.insert(index) {
            return Err(UnitArtifactJoinError::ArtifactClaimedTwice {
                path: artifact
                    .filenames
                    .iter()
                    .next()
                    .cloned()
                    .expect("CargoArtifactCollector requires filenames"),
            });
        }
        joined.insert(unit.unit, artifact);
    }
    Ok(joined)
}

/// Join every descriptor unit for which Cargo emitted a distinct
/// `compiler-artifact` message.
///
/// Host/target feature contexts without their own Cargo artifact are skipped;
/// they can still enter a root manifest only when an exact reachable
/// dependency artifact resolves them later.
pub fn join_cargo_enumerated_descriptor_units(
    units: &[ValidatedDescriptorUnit],
    artifacts: &[CargoCompilerArtifact],
) -> Result<BTreeMap<DescriptorCargoUnitId, CargoCompilerArtifact>, UnitArtifactJoinError> {
    let mut joined = BTreeMap::new();
    let mut claimed = BTreeMap::<
        (
            PathBuf,
            String,
            BTreeSet<String>,
            BTreeSet<String>,
            BTreeSet<PathBuf>,
        ),
        DescriptorCargoUnitId,
    >::new();

    for unit in units {
        let one =
            match join_descriptor_units_to_cargo_artifacts(std::slice::from_ref(unit), artifacts) {
                Ok(one) => one,
                // Cargo can invoke rustc for host/target feature contexts for
                // which it publishes no distinct `compiler-artifact` message.
                // Such units are not roots and cannot enter the root extern graph:
                // if a reachable `--extern` refers to one, dependency-hash
                // resolution below still fails closed.
                Err(UnitArtifactJoinError::MissingArtifact(_)) => continue,
                Err(error) => return Err(error),
            };
        let artifact = one
            .into_values()
            .next()
            .expect("one-unit join returns one artifact");
        let key = (
            artifact.manifest_path.clone(),
            artifact.target.name.clone(),
            artifact.target.kind.clone(),
            artifact.target.crate_types.clone(),
            artifact.filenames.clone(),
        );
        if claimed.insert(key, unit.unit).is_some() {
            return Err(UnitArtifactJoinError::ArtifactClaimedTwice {
                path: artifact
                    .filenames
                    .iter()
                    .next()
                    .cloned()
                    .expect("CargoArtifactCollector requires filenames"),
            });
        }
        joined.insert(unit.unit, artifact);
    }
    Ok(joined)
}

fn normalize_cargo_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

fn artifact_has_cargo_extra_filename(
    artifact: &CargoCompilerArtifact,
    crate_name: &str,
    cargo_extra_filename: &str,
) -> bool {
    artifact.filenames.iter().any(|path| {
        artifact_filename_has_cargo_extra_filename(path, crate_name, cargo_extra_filename)
    }) || artifact.executable.as_ref().is_some_and(|path| {
        artifact_filename_has_cargo_extra_filename(path, crate_name, cargo_extra_filename)
    })
}

fn artifact_filename_has_cargo_extra_filename(
    path: &Path,
    crate_name: &str,
    cargo_extra_filename: &str,
) -> bool {
    let Some(filename) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    [crate_name.to_string(), format!("lib{crate_name}")]
        .into_iter()
        .any(|prefix| {
            filename.strip_prefix(&prefix).is_some_and(|remainder| {
                remainder == cargo_extra_filename
                    || remainder
                        .strip_prefix(cargo_extra_filename)
                        .is_some_and(|extension| extension.starts_with('.'))
            })
        })
}

fn artifact_has_unhashed_primary_output(
    artifact: &CargoCompilerArtifact,
    crate_name: &str,
) -> bool {
    artifact
        .filenames
        .iter()
        .any(|path| artifact_filename_is_unhashed_primary_output(path, crate_name))
        || artifact.executable.as_ref().is_some_and(|path| {
            artifact.filenames.contains(path)
                && artifact_filename_is_unhashed_primary_output(path, crate_name)
        })
}

fn artifact_filename_is_unhashed_primary_output(path: &Path, crate_name: &str) -> bool {
    let Some(filename) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let normalized = normalize_cargo_crate_name(filename);
    normalized == crate_name
        || normalized
            .strip_prefix(crate_name)
            .is_some_and(|extension| extension.starts_with('.'))
        || normalized.strip_prefix("lib").is_some_and(|library| {
            library == crate_name
                || library
                    .strip_prefix(crate_name)
                    .is_some_and(|extension| extension.starts_with('.'))
        })
}

/// Build one root-specific external link manifest from exact Cargo messages,
/// wrapper sidecars, and descriptor objects.
pub(crate) fn build_link_unit_manifest(
    invocation: InvocationId,
    root_unit: DescriptorCargoUnitId,
    root_artifact: &Path,
    root_role: ArtifactRole,
    output: &DescriptorCargoOutput,
) -> Result<LinkUnitManifest, LinkManifestAssemblyError> {
    let topology = prepare_link_manifest(root_unit, root_artifact, root_role, output)?;
    let mut hasher = FilesystemLinkManifestHasher::new(
        root_artifact,
        &topology,
        &output.objects,
        &output.input_hashes,
    )?;
    assemble_link_unit_manifest(
        invocation,
        root_unit,
        root_artifact,
        root_role,
        &topology,
        &mut hasher,
    )
}

trait LinkManifestHasher {
    fn hash_single(&mut self, path: &Path, domain: &[u8]) -> io::Result<[u8; 32]>;

    fn hash_dependency(&mut self, path: &Path) -> io::Result<([u8; 32], [u8; 32])>;
}

struct FilesystemLinkManifestHasher {
    hashes: BTreeMap<PathBuf, ([u8; 32], [u8; 32])>,
    link_hashes: BTreeMap<PathBuf, [u8; 32]>,
}

impl FilesystemLinkManifestHasher {
    fn new(
        root_artifact: &Path,
        topology: &PreparedLinkManifest<'_>,
        objects: &BTreeMap<PathBuf, DescriptorObjectSnapshot>,
        input_hashes: &DescriptorInputHashes,
    ) -> Result<Self, LinkManifestAssemblyError> {
        let mut paths = BTreeSet::from([root_artifact.to_path_buf()]);
        let mut dependency_paths = BTreeSet::new();
        for unit in &topology.reachable {
            let descriptor = topology
                .unit_by_id
                .get(unit)
                .expect("prepared reachable units have descriptor records");
            if !objects.contains_key(&descriptor.descriptor_object) {
                paths.insert(descriptor.descriptor_object.clone());
            }
            for dependency in topology
                .dependencies_by_unit
                .get(unit)
                .expect("prepared reachable units have dependency records")
            {
                dependency_paths.insert(dependency.path.clone());
                paths.insert(dependency.path.clone());
            }
        }
        let paths = paths
            .into_iter()
            .filter(|path| !input_hashes.contains_key(path))
            .collect::<Vec<_>>();
        let hashed = paths
            .par_iter()
            .map(|path| {
                (
                    path.clone(),
                    if dependency_paths.contains(path) {
                        hash_file_two_domains(path, FILE_HASH_DOMAIN, LINK_ARTIFACT_HASH_DOMAIN)
                            .map(|(input, link)| (Some(input), link))
                    } else {
                        // Root and descriptor-only paths have no extern input
                        // expectation. Do not compute a digest nobody uses.
                        hash_file(path, LINK_ARTIFACT_HASH_DOMAIN).map(|link| (None, link))
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut hashes = input_hashes.clone();
        let mut link_hashes = objects
            .iter()
            .map(|(path, object)| (path.clone(), object.link_hash))
            .collect::<BTreeMap<_, _>>();
        for (path, result) in hashed {
            let hash = result.map_err(|source| LinkManifestAssemblyError::HashArtifact {
                path: path.clone(),
                source,
            })?;
            if let Some(input) = hash.0 {
                hashes.insert(path, (input, hash.1));
            } else {
                link_hashes.insert(path, hash.1);
            }
        }
        Ok(Self {
            hashes,
            link_hashes,
        })
    }

    fn hashes(&self, path: &Path) -> io::Result<([u8; 32], [u8; 32])> {
        self.hashes.get(path).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "link manifest requested an unplanned artifact `{}`",
                    path.display()
                ),
            )
        })
    }
}

impl LinkManifestHasher for FilesystemLinkManifestHasher {
    fn hash_single(&mut self, path: &Path, domain: &[u8]) -> io::Result<[u8; 32]> {
        if domain == LINK_ARTIFACT_HASH_DOMAIN {
            if let Some(hash) = self.link_hashes.get(path) {
                return Ok(*hash);
            }
        }
        let (input, link) = self.hashes(path)?;
        if domain == FILE_HASH_DOMAIN {
            Ok(input)
        } else if domain == LINK_ARTIFACT_HASH_DOMAIN {
            Ok(link)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "link manifest requested an unsupported hash domain",
            ))
        }
    }

    fn hash_dependency(&mut self, path: &Path) -> io::Result<([u8; 32], [u8; 32])> {
        self.hashes(path)
    }
}

#[derive(Clone)]
struct RealizedLinkDependency {
    to: DescriptorCargoUnitId,
    extern_name: String,
    path: PathBuf,
    role: ArtifactRole,
    expected_input_hash: String,
}

struct PreparedLinkManifest<'a> {
    unit_by_id: BTreeMap<DescriptorCargoUnitId, &'a ValidatedDescriptorUnit>,
    dependencies_by_unit: BTreeMap<DescriptorCargoUnitId, Vec<RealizedLinkDependency>>,
    reachable: BTreeSet<DescriptorCargoUnitId>,
}

#[cfg(test)]
fn build_link_unit_manifest_with_hasher(
    invocation: InvocationId,
    root_unit: DescriptorCargoUnitId,
    root_artifact: &Path,
    root_role: ArtifactRole,
    output: &DescriptorCargoOutput,
    hasher: &mut impl LinkManifestHasher,
) -> Result<LinkUnitManifest, LinkManifestAssemblyError> {
    let topology = prepare_link_manifest(root_unit, root_artifact, root_role, output)?;
    assemble_link_unit_manifest(
        invocation,
        root_unit,
        root_artifact,
        root_role,
        &topology,
        hasher,
    )
}

fn prepare_link_manifest<'a>(
    root_unit: DescriptorCargoUnitId,
    root_artifact: &Path,
    root_role: ArtifactRole,
    output: &'a DescriptorCargoOutput,
) -> Result<PreparedLinkManifest<'a>, LinkManifestAssemblyError> {
    if !root_role.is_root() {
        return Err(LinkManifestAssemblyError::NonRootRole(root_role));
    }
    if !root_artifact.is_absolute() {
        return Err(LinkManifestAssemblyError::RelativeRootArtifact(
            root_artifact.to_path_buf(),
        ));
    }
    let joined = join_cargo_enumerated_descriptor_units(&output.units, &output.cargo_artifacts)
        .map_err(LinkManifestAssemblyError::Join)?;
    let root_cargo_artifact = joined
        .get(&root_unit)
        .ok_or(LinkManifestAssemblyError::UnknownRootUnit(root_unit))?;
    let root_is_enumerated = root_cargo_artifact.filenames.contains(root_artifact)
        || root_cargo_artifact.executable.as_deref() == Some(root_artifact);
    if !root_is_enumerated {
        return Err(LinkManifestAssemblyError::RootArtifactNotEnumerated {
            unit: root_unit,
            path: root_artifact.to_path_buf(),
        });
    }

    let unit_by_id = output
        .units
        .iter()
        .map(|unit| (unit.unit, unit))
        .collect::<BTreeMap<_, _>>();
    let mut artifact_by_path =
        BTreeMap::<PathBuf, Vec<(DescriptorCargoUnitId, ArtifactRole)>>::new();
    for (unit, artifact) in &joined {
        for path in &artifact.filenames {
            if !path.is_file() {
                return Err(LinkManifestAssemblyError::MissingCargoFilename(
                    path.clone(),
                ));
            }
            if let Some(role) = classify_dependency_artifact(artifact, path) {
                artifact_by_path
                    .entry(path.clone())
                    .or_default()
                    .push((*unit, role));
            }
        }
        if let Some(path) = &artifact.executable {
            if !artifact.filenames.contains(path) && path.is_file() {
                artifact_by_path
                    .entry(path.clone())
                    .or_default()
                    .push((*unit, ArtifactRole::RootExecutable));
            }
        }
        if let Some(path) = uplifted_root_lib_hashed_rlib(artifact) {
            artifact_by_path
                .entry(path)
                .or_default()
                .push((*unit, ArtifactRole::RustLibrary));
        }
    }
    for candidates in artifact_by_path.values_mut() {
        candidates.sort_by(|left, right| {
            left.0
                .as_bytes()
                .cmp(right.0.as_bytes())
                .then_with(|| left.1.cmp(&right.1))
        });
        candidates.dedup();
    }

    let mut dependencies_by_unit =
        BTreeMap::<DescriptorCargoUnitId, Vec<RealizedLinkDependency>>::new();
    for unit in &output.units {
        let mut dependencies = Vec::new();
        for (extern_name, dependency) in &unit.sidecar.dependency_artifacts {
            let candidates = artifact_by_path.get(&dependency.path).ok_or_else(|| {
                LinkManifestAssemblyError::UnresolvedDependencyArtifact {
                    unit: unit.unit,
                    extern_name: extern_name.clone(),
                    path: dependency.path.clone(),
                    artifact_hash: dependency.content_hash.clone(),
                }
            })?;
            let (to, role) = match candidates.as_slice() {
                [(to, role)] => (*to, *role),
                _ => {
                    return Err(LinkManifestAssemblyError::AmbiguousDependencyArtifact {
                        unit: unit.unit,
                        extern_name: extern_name.clone(),
                        path: dependency.path.clone(),
                        artifact_hash: dependency.content_hash.clone(),
                        matches: candidates.len(),
                    });
                }
            };
            if to == unit.unit {
                return Err(LinkManifestAssemblyError::SelfDependency {
                    unit: unit.unit,
                    extern_name: extern_name.clone(),
                });
            }
            dependencies.push(RealizedLinkDependency {
                to,
                extern_name: extern_name.clone(),
                path: dependency.path.clone(),
                role,
                expected_input_hash: dependency.content_hash.clone(),
            });
        }
        dependencies.sort_by(|left, right| {
            left.extern_name
                .cmp(&right.extern_name)
                .then_with(|| left.to.as_bytes().cmp(right.to.as_bytes()))
        });
        dependencies_by_unit.insert(unit.unit, dependencies);
    }

    let mut reachable = BTreeSet::new();
    let mut pending = vec![root_unit];
    while let Some(unit) = pending.pop() {
        if !reachable.insert(unit) {
            continue;
        }
        let dependencies = dependencies_by_unit
            .get(&unit)
            .ok_or(LinkManifestAssemblyError::UnknownReachableUnit(unit))?;
        for dependency in dependencies.iter().rev() {
            pending.push(dependency.to);
        }
    }

    Ok(PreparedLinkManifest {
        unit_by_id,
        dependencies_by_unit,
        reachable,
    })
}

fn assemble_link_unit_manifest(
    invocation: InvocationId,
    root_unit: DescriptorCargoUnitId,
    root_artifact: &Path,
    root_role: ArtifactRole,
    topology: &PreparedLinkManifest<'_>,
    hasher: &mut impl LinkManifestHasher,
) -> Result<LinkUnitManifest, LinkManifestAssemblyError> {
    let PreparedLinkManifest {
        unit_by_id,
        dependencies_by_unit,
        reachable,
    } = topology;

    let mut artifacts = Vec::new();
    let mut dependency_edges = BTreeSet::new();
    let mut included_dependency_artifacts =
        BTreeSet::<(DescriptorCargoUnitId, ArtifactRole, SemanticHash)>::new();
    let mut dependency_content = BTreeMap::<PathBuf, ([u8; 32], SemanticHash)>::new();
    for unit in reachable {
        let descriptor = unit_by_id
            .get(unit)
            .ok_or(LinkManifestAssemblyError::UnknownReachableUnit(*unit))?;
        let object_hash = hasher
            .hash_single(&descriptor.descriptor_object, LINK_ARTIFACT_HASH_DOMAIN)
            .map_err(|source| LinkManifestAssemblyError::HashArtifact {
                path: descriptor.descriptor_object.clone(),
                source,
            })?;
        artifacts.push(LinkArtifact {
            unit: unit.as_abi_id(),
            role: ArtifactRole::DescriptorObject,
            content: SemanticHash::from_bytes(object_hash),
            path: descriptor.descriptor_object.to_string_lossy().into_owned(),
        });

        for dependency in dependencies_by_unit
            .get(unit)
            .expect("reachable units were validated")
        {
            if !reachable.contains(&dependency.to) {
                return Err(LinkManifestAssemblyError::ReachabilityInvariant {
                    from: *unit,
                    to: dependency.to,
                });
            }
            dependency_edges.insert(LinkUnitDependency {
                from: unit.as_abi_id(),
                to: dependency.to.as_abi_id(),
                extern_name: dependency.extern_name.clone(),
            });
            let (input_hash, content) = match dependency_content.get(&dependency.path) {
                Some(content) => *content,
                None => {
                    let (input_hash, link_hash) = hasher
                        .hash_dependency(&dependency.path)
                        .map_err(|source| LinkManifestAssemblyError::HashArtifact {
                            path: dependency.path.clone(),
                            source,
                        })?;
                    let content = (input_hash, SemanticHash::from_bytes(link_hash));
                    dependency_content.insert(dependency.path.clone(), content);
                    content
                }
            };
            let actual_input_hash = hex_digest(&input_hash);
            if actual_input_hash != dependency.expected_input_hash {
                return Err(LinkManifestAssemblyError::DependencyArtifactHashMismatch {
                    unit: *unit,
                    extern_name: dependency.extern_name.clone(),
                    path: dependency.path.clone(),
                    expected: dependency.expected_input_hash.clone(),
                    actual: actual_input_hash,
                });
            }
            if included_dependency_artifacts.insert((dependency.to, dependency.role, content)) {
                artifacts.push(LinkArtifact {
                    unit: dependency.to.as_abi_id(),
                    role: dependency.role,
                    content,
                    path: dependency.path.to_string_lossy().into_owned(),
                });
            }
        }
    }

    let root_content = hasher
        .hash_single(root_artifact, LINK_ARTIFACT_HASH_DOMAIN)
        .map_err(|source| LinkManifestAssemblyError::HashArtifact {
            path: root_artifact.to_path_buf(),
            source,
        })?;
    artifacts.push(LinkArtifact {
        unit: root_unit.as_abi_id(),
        role: root_role,
        content: SemanticHash::from_bytes(root_content),
        path: root_artifact.to_string_lossy().into_owned(),
    });
    artifacts.sort_by(|left, right| {
        left.unit
            .as_bytes()
            .cmp(right.unit.as_bytes())
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.content.as_bytes().cmp(right.content.as_bytes()))
    });

    let manifest = LinkUnitManifest {
        invocation,
        root_unit: root_unit.as_abi_id(),
        artifacts,
        dependencies: dependency_edges,
    };
    manifest
        .validate()
        .map_err(LinkManifestAssemblyError::InvalidManifest)?;
    Ok(manifest)
}

/// Cargo reports a root package's lib artifact at its uplifted destination
/// (`target/debug/libX.rlib`) plus the hashed `deps/libX-<hash>.rmeta`, while
/// same-package dependents are compiled against the hashed
/// `deps/libX-<hash>.rlib` that the message never lists. Derive that hashed
/// rlib from the message's own hashed rmeta filename; both names come from one
/// compiler-artifact message, so the pairing cannot cross units.
pub(crate) fn uplifted_root_lib_hashed_rlib(artifact: &CargoCompilerArtifact) -> Option<PathBuf> {
    if !artifact.target.crate_types.contains("lib") {
        return None;
    }
    let crate_name = normalize_cargo_crate_name(&artifact.target.name);
    let uplifted = format!("lib{crate_name}.rlib");
    artifact
        .filenames
        .iter()
        .find(|path| path.file_name().and_then(OsStr::to_str) == Some(uplifted.as_str()))?;
    let hashed_prefix = format!("lib{crate_name}-");
    let rmeta = artifact.filenames.iter().find(|path| {
        path.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(&hashed_prefix) && name.ends_with(".rmeta"))
    })?;
    let hashed_rlib = format!(
        "{}.rlib",
        rmeta.file_name()?.to_str()?.strip_suffix(".rmeta")?
    );
    let candidate = rmeta.parent()?.join(hashed_rlib);
    candidate.is_file().then_some(candidate)
}

pub(crate) fn classify_dependency_artifact(
    artifact: &CargoCompilerArtifact,
    path: &Path,
) -> Option<ArtifactRole> {
    if artifact.target.kind.contains("proc-macro") {
        return Some(ArtifactRole::ProcMacro);
    }
    match path.extension().and_then(OsStr::to_str) {
        Some("rlib") => Some(ArtifactRole::RustLibrary),
        Some("rmeta") => Some(ArtifactRole::RustMetadata),
        Some("a") | Some("lib") => Some(ArtifactRole::StaticLibrary),
        Some("so") | Some("dylib") | Some("dll") => Some(ArtifactRole::DynamicLibrary),
        Some("o") | Some("obj") => Some(ArtifactRole::Object),
        _ => None,
    }
}

/// Failure while constructing one root's exact external link manifest.
#[derive(Debug, Error)]
pub enum LinkManifestAssemblyError {
    /// Caller supplied a dependency role for the selected root.
    #[error("artifact role {0:?} is not a root role")]
    NonRootRole(ArtifactRole),
    /// Root path must come directly from Cargo.
    #[error("root artifact path must be absolute: `{0}`")]
    RelativeRootArtifact(PathBuf),
    /// Sidecar/Cargo freshness join failed.
    #[error(transparent)]
    Join(UnitArtifactJoinError),
    /// Root unit was not part of the joined invocation.
    #[error("selected root Cargo unit {0} was not produced by this descriptor invocation")]
    UnknownRootUnit(DescriptorCargoUnitId),
    /// Root path was not in that unit's Cargo message.
    #[error("root artifact `{path}` was not enumerated for Cargo unit {unit}")]
    RootArtifactNotEnumerated {
        /// Selected unit.
        unit: DescriptorCargoUnitId,
        /// Exact path.
        path: PathBuf,
    },
    /// Cargo advertised a missing file.
    #[error("Cargo-enumerated artifact is missing: `{0}`")]
    MissingCargoFilename(PathBuf),
    /// Exact artifact hashing failed.
    #[error("failed to hash link artifact `{path}`: {source}")]
    HashArtifact {
        /// Exact path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// An extern input did not correspond to a sidecar-backed Cargo artifact.
    #[error(
        "descriptor unit {unit} extern `{extern_name}` references Cargo-unenumerated artifact `{path}` with hash `{artifact_hash}`"
    )]
    UnresolvedDependencyArtifact {
        /// Consumer.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
        /// Exact path passed to rustc.
        path: PathBuf,
        /// Wrapper input hash.
        artifact_hash: String,
    },
    /// An exact dependency path was enumerated by multiple units.
    #[error(
        "descriptor unit {unit} extern `{extern_name}` artifact `{path}` with hash `{artifact_hash}` matches {matches} Cargo units"
    )]
    AmbiguousDependencyArtifact {
        /// Consumer.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
        /// Exact path passed to rustc.
        path: PathBuf,
        /// Wrapper input hash.
        artifact_hash: String,
        /// Candidate count.
        matches: usize,
    },
    /// A Cargo-enumerated extern changed after the wrapper captured its input.
    #[error(
        "descriptor unit {unit} extern `{extern_name}` artifact `{path}` changed after descriptor input capture: expected hash `{expected}`, found `{actual}`"
    )]
    DependencyArtifactHashMismatch {
        /// Consumer.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
        /// Exact Cargo-enumerated path.
        path: PathBuf,
        /// Wrapper-time descriptor-domain hash.
        expected: String,
        /// Assembly-time descriptor-domain hash.
        actual: String,
    },
    /// Cargo graph must not self-reference.
    #[error("descriptor unit {unit} extern `{extern_name}` resolves to itself")]
    SelfDependency {
        /// Consumer.
        unit: DescriptorCargoUnitId,
        /// Rust extern name.
        extern_name: String,
    },
    /// Reached an ID with no wrapper sidecar.
    #[error("descriptor root graph reaches unknown Cargo unit {0}")]
    UnknownReachableUnit(DescriptorCargoUnitId),
    /// Internal closure bug.
    #[error("descriptor reachability omitted dependency {from} -> {to}")]
    ReachabilityInvariant {
        /// Consumer.
        from: DescriptorCargoUnitId,
        /// Dependency.
        to: DescriptorCargoUnitId,
    },
    /// Canonical manifest validation failed.
    #[error("assembled link-unit manifest is invalid: {0}")]
    InvalidManifest(rsjs_ir::build::BuildIdentityError),
}

/// Freshness-join failure between wrapper sidecars and Cargo JSON.
#[derive(Debug, Error)]
pub enum UnitArtifactJoinError {
    /// Direct rustc invocation lacks Cargo provenance and cannot enter a root
    /// manifest.
    #[error("descriptor unit {0} has no Cargo manifest provenance")]
    MissingCargoManifest(DescriptorCargoUnitId),
    /// Cargo manifest provenance must be absolute.
    #[error("descriptor unit {unit} reports relative Cargo manifest `{path}`")]
    RelativeCargoManifest {
        /// Unit being joined.
        unit: DescriptorCargoUnitId,
        /// Invalid path.
        path: PathBuf,
    },
    /// Sidecar normalization failed closed.
    #[error("descriptor unit {0} repeats a crate type")]
    DuplicateCrateType(DescriptorCargoUnitId),
    /// Sidecar normalization failed closed.
    #[error("descriptor unit {0} repeats a Cargo feature")]
    DuplicateFeature(DescriptorCargoUnitId),
    /// Output discriminators are exact filename fragments, never paths.
    #[error("descriptor unit {unit} has invalid Cargo extra-filename `{value}`: {reason}")]
    InvalidCargoExtraFilename {
        /// Unit being joined.
        unit: DescriptorCargoUnitId,
        /// Rejected discriminator.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cargo source bytes could not be validated.
    #[error("failed to hash Cargo source `{path}` while joining descriptor units: {source}")]
    HashCargoSource {
        /// Exact Cargo source path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// No Cargo message describes this exact unit.
    #[error("no Cargo compiler-artifact matches descriptor unit {0}")]
    MissingArtifact(DescriptorCargoUnitId),
    /// More than one Cargo message could authorize this unit.
    #[error("descriptor unit {unit} ambiguously matches {matches} Cargo compiler-artifacts")]
    AmbiguousArtifact {
        /// Unit being joined.
        unit: DescriptorCargoUnitId,
        /// Candidate count.
        matches: usize,
    },
    /// One Cargo message cannot authorize two wrapper units.
    #[error("Cargo compiler-artifact `{path}` was claimed by multiple descriptor units")]
    ArtifactClaimedTwice {
        /// Representative exact filename.
        path: PathBuf,
    },
}

/// Hidden rustc-wrapper failure.
#[derive(Debug, Error)]
pub enum WrapperError {
    /// Descriptor carrier cannot safely collect every codegen object.
    #[error("descriptor carrier: {0}")]
    DescriptorCarrier(String),
    /// Required wrapper configuration is missing.
    #[error("missing required RSJS wrapper environment variable `{0}`")]
    MissingEnvironment(&'static str),
    /// Process environment names must be deterministic UTF-8 hash inputs.
    #[error("RSJS wrapper environment contains a non-UTF-8 variable name: {0:?}")]
    NonUtf8EnvironmentName(OsString),
    /// Process environment values must be deterministic UTF-8 hash inputs.
    #[error("RSJS wrapper environment variable `{name}` has a non-UTF-8 value: {value:?}")]
    NonUtf8EnvironmentValue {
        /// Variable name.
        name: String,
        /// Rejected value.
        value: OsString,
    },
    /// A future/unknown wrapper protocol was requested.
    #[error("unsupported RSJS rustc-wrapper mode `{0}`")]
    UnsupportedWrapperMode(String),
    /// Relative staging paths would depend on Cargo's invocation cwd.
    #[error("descriptor staging directory must be absolute: `{0}`")]
    RelativeStagingDirectory(PathBuf),
    /// Descriptor source-closure exclusion must name the exact Cargo target.
    #[error("descriptor target directory must be absolute: `{0}`")]
    RelativeDescriptorTargetDirectory(PathBuf),
    /// Package source exclusion must use one exact absolute generated root.
    #[error("descriptor source exclusion directory must be absolute: `{0}`")]
    RelativeSourceExclusionDirectory(PathBuf),
    /// Package closure identity requires one absolute Cargo package root.
    #[error("Cargo manifest directory must be absolute: `{0}`")]
    RelativeCargoManifestDirectory(PathBuf),
    /// Final maps are resolved before Cargo starts and must be exact.
    #[error("final unit-map path must be absolute: `{0}`")]
    RelativeFinalUnitMap(PathBuf),
    /// Final target confinement begins at one exact absolute runner path.
    #[error("final Cargo target must be absolute: `{0}`")]
    RelativeFinalTarget(PathBuf),
    /// The runner-owned final target must remain a plain directory.
    #[error("unsafe final Cargo target `{0}`")]
    UnsafeFinalTarget(PathBuf),
    /// Cargo OUT_DIR must be a strict lexical descendant of the final target.
    #[error("Cargo OUT_DIR `{out_dir}` is outside final target `{target}`")]
    OutDirOutsideFinalTarget {
        /// Cargo-provided build output directory.
        out_dir: PathBuf,
        /// Exact invocation- and wrapper-policy-scoped final target.
        target: PathBuf,
    },
    /// Proc macros consume one exact content-addressed final plan.
    #[error("final link-plan path must be absolute: `{0}`")]
    RelativeFinalPlan(PathBuf),
    /// Final mode must be explicitly authorized by the orchestrator.
    #[error("RSJS final wrapper requires `RSJS_FINAL_PHASE=1`")]
    InvalidFinalPhase,
    /// Hash inputs use one exact lowercase encoding.
    #[error("invalid SHA-256 in `{name}` (`{value}`): {reason}")]
    InvalidSha256 {
        /// Environment or field name.
        name: &'static str,
        /// Rejected value.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Cargo's rustc output discriminator must be one safe filename fragment.
    #[error("invalid Cargo extra-filename in {context} (`{value}`): {reason}")]
    InvalidCargoExtraFilename {
        /// Validation boundary.
        context: &'static str,
        /// Rejected discriminator.
        value: String,
        /// Validation detail.
        reason: String,
    },
    /// Content-addressed filename did not match the expected hash.
    #[error("final unit map `{path}` must be named `{expected}`")]
    FinalUnitMapFilename {
        /// Exact path.
        path: PathBuf,
        /// Expected filename.
        expected: String,
    },
    /// Map bytes could not be read.
    #[error("failed to read final unit map `{path}`: {source}")]
    ReadFinalUnitMap {
        /// Exact path.
        path: PathBuf,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Content-addressed map bytes changed.
    #[error("final unit map `{path}` has SHA-256 `{actual}`; expected `{expected}`")]
    FinalUnitMapHashMismatch {
        /// Exact path.
        path: PathBuf,
        /// Required digest.
        expected: String,
        /// Actual digest.
        actual: String,
    },
    /// Map JSON was malformed.
    #[error("failed to parse final unit map `{path}`: {source}")]
    ParseFinalUnitMap {
        /// Exact path.
        path: PathBuf,
        /// JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// Wrong external map format.
    #[error("unsupported final unit-map format `{0}`")]
    FinalUnitMapFormat(String),
    /// Wrong map schema.
    #[error("final unit-map schema {actual} is unsupported; expected {expected}")]
    FinalUnitMapSchema {
        /// Actual version.
        actual: u16,
        /// Required version.
        expected: u16,
    },
    /// Map and final compilation named different plans.
    #[error("final unit-map plan ID mismatch: expected `{expected}`, found `{actual}`")]
    FinalUnitMapPlanId {
        /// Environment plan.
        expected: String,
        /// Map plan.
        actual: String,
    },
    /// Map and final compilation named different exact plan files.
    #[error(
        "final unit-map linked-plan SHA-256 mismatch: expected `{expected}`, found `{actual}`"
    )]
    FinalUnitMapPlanFileHash {
        /// Environment hash.
        expected: String,
        /// Map hash.
        actual: String,
    },
    /// Serialized entries must be strictly coordinate-sorted.
    #[error("final unit-map entries are not in strict canonical coordinate order")]
    NonCanonicalFinalUnitMap,
    /// Deserialized coordinate must remain phase invariant.
    #[error("final unit-map coordinate has relative manifest path `{0}`")]
    RelativeMappedManifest(PathBuf),
    /// Set-valued coordinate field was duplicated or unsorted.
    #[error("final unit-map coordinate has non-canonical `{0}` values")]
    NonCanonicalFinalCoordinate(&'static str),
    /// Optional profile names cannot use an empty sentinel.
    #[error("final unit-map coordinate contains an empty Cargo profile")]
    EmptyFinalProfile,
    /// Descriptor coordinates cannot contain final cfgs.
    #[error("final unit-map descriptor coordinate contains a final-plan cfg")]
    FinalCoordinateContainsPlanCfg,
    /// Map feature list disagreed with its cfgs.
    #[error("final unit-map coordinate feature list disagrees with its cfgs")]
    FinalCoordinateFeatureMismatch,
    /// Two entries would let wrapper order choose identity.
    #[error(
        "final coordinate {coordinate:?} ambiguously maps to Cargo units {first} and {second}"
    )]
    AmbiguousFinalUnitCoordinate {
        /// Ambiguous coordinate.
        coordinate: FinalUnitCoordinate,
        /// First unit.
        first: String,
        /// Second unit.
        second: String,
    },
    /// Cargo-owned inputs may not contain a wrapper-owned final cfg.
    #[error("Cargo's rustc invocation contains reserved plan cfg `{0}` before wrapper injection")]
    UnexpectedFinalPlanCfg(String),
    /// Cargo did not provide the real rustc executable as `argv[1]`.
    #[error("Cargo's rustc-wrapper invocation did not include the rustc executable")]
    MissingRustcProgram,
    /// Cargo silently composes a configured workspace wrapper inside Proa's
    /// wrapper. That extra program is not part of descriptor/final identity.
    #[error(
        "Cargo composed unsupported `build.rustc-workspace-wrapper` program {wrapper:?} before rustc {rustc:?}; remove that Cargo setting before invoking cargo-proa"
    )]
    ComposedWorkspaceWrapper {
        /// Workspace wrapper inserted by Cargo.
        wrapper: OsString,
        /// Actual rustc program nested inside the workspace wrapper.
        rustc: OsString,
    },
    /// rustc could not be launched.
    #[error("failed to run rustc `{program}`: {source}")]
    SpawnRustc {
        /// Program supplied by Cargo.
        program: PathBuf,
        /// Process error.
        #[source]
        source: io::Error,
    },
    /// rustc version material could not be obtained.
    #[error("`{program} -vV` failed with {status}")]
    RustcIdentityFailed {
        /// Program supplied by Cargo.
        program: PathBuf,
        /// Failed exit status.
        status: ExitStatus,
    },
    /// rustc option lacks a required following value.
    #[error("rustc option `{0}` is missing its value")]
    MissingRustcOptionValue(&'static str),
    /// Cargo compilation must request one dep-info output for exact generated
    /// source auditing.
    #[error("Cargo's rustc invocation did not request `--emit=dep-info`")]
    MissingDepInfoOutput,
    /// Multiple dep-info destinations would make the audited source set
    /// order-dependent.
    #[error("Cargo's rustc invocation requested more than one dep-info output")]
    DuplicateDepInfoOutput,
    /// A default dep-info path cannot be inferred safely when rustc also owns
    /// a single explicit output path.
    #[error("Cargo's rustc invocation combines default dep-info with `-o`; an explicit dep-info path is required")]
    UnsupportedDefaultDepInfoWithOutput,
    /// The wrapper could not resolve a relative rustc output path.
    #[error("failed to resolve the rustc wrapper current directory: {0}")]
    CurrentDirectory(io::Error),
    /// Paths may be non-UTF-8, but rustc option spelling must be unambiguous.
    #[error("descriptor wrapper does not support this non-UTF-8 rustc argument: {0:?}")]
    NonUtf8RustcArgument(OsString),
    /// A future rustc option must be classified before entering unit identity.
    #[error("descriptor wrapper does not yet classify rustc option `{0}`")]
    UnsupportedRustcOption(String),
    /// Binary dependency dep-info would bind descriptor/final target-specific
    /// rlibs, metadata, and sysroot artifacts into the source protocol.
    #[error("`-Zbinary-dep-depinfo` is unsupported by the exact observed-input protocol")]
    UnsupportedBinaryDepDepInfo,
    /// rustc's logical environment injection is not yet modeled separately
    /// from the wrapper process environment.
    #[error("`--env-set` is unsupported by the exact observed-input protocol")]
    UnsupportedLogicalEnvironmentInjection,
    /// rustc remapping changes dep-info path spelling, so the strict v1
    /// generated-input audit cannot resolve those paths back to exact files.
    #[error("rustc path remapping option `{0}` is unsupported by the exact observed-input audit")]
    UnsupportedDepInfoPathRemapping(String),
    /// No `--crate-name` was present.
    #[error("descriptor rustc invocation has no `--crate-name`")]
    MissingCrateName,
    /// No crate type was present.
    #[error("descriptor rustc invocation has no `--crate-type`")]
    MissingCrateType,
    /// No source input was present.
    #[error("descriptor rustc invocation has no source input")]
    MissingRustcInput,
    /// More than one positional source was present.
    #[error("descriptor rustc invocation has multiple positional source inputs")]
    MultipleRustcInputs,
    /// `--extern` syntax is incomplete.
    #[error("malformed rustc `--extern` value `{0}`")]
    MalformedExtern(String),
    /// Two paths for the same extern name had different bytes.
    #[error("rustc invocation contains conflicting artifacts for extern `{0}`")]
    ConflictingExtern(String),
    /// Exact Cargo joins require the absolute path Cargo reports in JSON.
    #[error("rustc invocation contains relative extern artifact `{0}`")]
    RelativeExternArtifact(PathBuf),
    /// A source or exact dependency artifact could not be hashed.
    #[error("failed to hash exact descriptor input `{path}`: {source}")]
    HashInput {
        /// Exact input path.
        path: PathBuf,
        /// Read error.
        #[source]
        source: io::Error,
    },
    /// The conservative package-local source closure could not be hashed.
    #[error("failed to hash Cargo package source closure `{path}`: {source}")]
    HashPackageSourceClosure {
        /// Exact Cargo package root.
        path: PathBuf,
        /// Traversal or read error.
        #[source]
        source: io::Error,
    },
    /// Cargo build-script output directories must be absolute.
    #[error("Cargo OUT_DIR must be absolute: `{0}`")]
    RelativeOutDir(PathBuf),
    /// Relative dep-info bases would make cached source resolution depend on
    /// the caller process cwd.
    #[error("rustc dep-info working directory must be absolute: `{0}`")]
    RelativeDepInfoWorkingDirectory(PathBuf),
    /// The descriptor OUT_DIR must be owned by the exact descriptor target.
    #[error("Cargo OUT_DIR `{out_dir}` is outside descriptor target `{target}`")]
    OutDirOutsideDescriptorTarget {
        /// Cargo-provided build output directory.
        out_dir: PathBuf,
        /// Orchestrator-owned target directory.
        target: PathBuf,
    },
    /// rustc's exact dep-info could not be read.
    #[error("failed to read rustc dep-info `{path}`: {source}")]
    ReadDepInfo {
        /// Exact dep-info path.
        path: PathBuf,
        /// Read failure.
        #[source]
        source: io::Error,
    },
    /// A stale dep-info output could not be removed before rustc.
    #[error("failed to prepare rustc dep-info output `{path}`: {source}")]
    WriteDepInfo {
        /// Exact dep-info path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Dep-info paths must remain regular files owned by rustc.
    #[error("unsafe rustc dep-info output `{0}`")]
    UnsafeDepInfoOutput(PathBuf),
    /// rustc's dep-info encoding cannot represent this path without
    /// ambiguity (non-UTF-8, backslash, non-space whitespace, or trailing
    /// space on a file).
    #[error("path `{0}` cannot be represented unambiguously in rustc dep-info")]
    UnrepresentableDepInfoPath(PathBuf),
    /// The wrapper may remove only Cargo target-owned dep-info outputs.
    #[error("rustc dep-info output `{path}` is outside exact target `{target}`")]
    DepInfoOutsideTarget {
        /// Rejected dep-info path.
        path: PathBuf,
        /// Orchestrator-owned target.
        target: PathBuf,
    },
    /// rustc's dep-info was not canonical Makefile dependency syntax.
    #[error("malformed rustc dep-info `{path}`: {reason}")]
    MalformedDepInfo {
        /// Exact dep-info path.
        path: PathBuf,
        /// Parse detail.
        reason: String,
    },
    /// A generated source input could not be inspected or hashed.
    #[error("failed to inspect rustc-declared OUT_DIR dependency `{path}`: {source}")]
    ReadOutDirDependency {
        /// Exact generated input.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Generated source inputs must be regular files beneath one exact OUT_DIR.
    #[error("unsafe rustc-declared OUT_DIR dependency `{0}`")]
    UnsafeOutDirDependency(PathBuf),
    /// One dep-info file cannot report conflicting bytes for the same portable
    /// generated-source path.
    #[error("conflicting rustc-declared OUT_DIR dependency `{0}`")]
    ConflictingOutDirDependency(String),
    /// OUT_DIR presence, its portable coordinate, and its live path must agree.
    #[error("inconsistent OUT_DIR state in observed-input metadata")]
    InconsistentObservedOutDir,
    /// A serialized OUT_DIR coordinate is not a portable relative path.
    #[error("invalid observed OUT_DIR coordinate: {0}")]
    InvalidObservedOutDirCoordinate(String),
    /// Serialized generated-source paths are strict portable relative paths.
    #[error("invalid OUT_DIR dependency path `{path}`: {reason}")]
    InvalidOutDirDependencyPath {
        /// Rejected path.
        path: String,
        /// Validation detail.
        reason: String,
    },
    /// Dependency records are sorted and unique so map order cannot select
    /// behavior.
    #[error("OUT_DIR dependency records are not strictly sorted and unique")]
    NonCanonicalOutDirDependencies,
    /// Non-OUT_DIR observed source paths must be canonical absolute paths.
    #[error("observed source dependency path must be absolute: `{0}`")]
    RelativeObservedSource(PathBuf),
    /// An observed source could not be inspected or hashed.
    #[error("failed to inspect rustc-declared source dependency `{path}`: {source}")]
    ReadObservedSource {
        /// Exact source path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Symlinks and special files are not stable exact source inputs.
    #[error("unsafe rustc-declared source dependency `{0}`")]
    UnsafeObservedSource(PathBuf),
    /// Canonical aliases cannot appear twice in one exact source set.
    #[error("conflicting rustc-declared source dependency `{0}`")]
    ConflictingObservedSource(PathBuf),
    /// Absolute source records are sorted and unique.
    #[error("observed source dependency records are not strictly sorted and unique")]
    NonCanonicalSourceDependencies,
    /// Live source bytes changed after descriptor observation.
    #[error("source file changed between RSJS compilation passes: `{path}`")]
    ObservedSourceHashMismatch {
        /// Exact source path.
        path: PathBuf,
        /// Descriptor-time hash.
        expected: String,
        /// Current hash.
        actual: String,
    },
    /// Environment records are sorted, unique, and cannot contain wrapper
    /// control names.
    #[error("observed environment dependency records are not strictly canonical")]
    NonCanonicalEnvironmentDependencies,
    /// Environment names must be nonempty and cannot be phase-owned controls.
    #[error("invalid observed environment dependency name `{name}`")]
    InvalidEnvironmentDependencyName {
        /// Rejected name. Values are never included.
        name: String,
    },
    /// One dep-info file declared the same environment variable twice.
    #[error("rustc dep-info repeats environment dependency `{name}`")]
    DuplicateEnvironmentDependency {
        /// Repeated name. Values are never included.
        name: String,
    },
    /// Current user/Cargo environment presence or value hash diverged.
    #[error("observed environment dependency `{name}` differs from descriptor compilation")]
    ObservedEnvironmentMismatch {
        /// Divergent name. Values are deliberately redacted.
        name: String,
    },
    /// A compiler-tracked wrapper control disagreed with the exact child
    /// environment. Its value remains redacted.
    #[error("phase-owned environment dependency `{name}` disagrees with the wrapper protocol")]
    PhaseOwnedEnvironmentMismatch {
        /// Divergent phase-owned name.
        name: String,
    },
    /// Phase-owned paths/IDs must be UTF-8 before rustc can report them in its
    /// UTF-8 dep-info protocol.
    #[error("phase-owned environment dependency `{0}` is not UTF-8")]
    NonUtf8PhaseOwnedEnvironment(&'static str),
    /// A mapped unit that consumed generated source requires Cargo's OUT_DIR.
    #[error("mapped final unit consumed OUT_DIR dependencies but final rustc has no OUT_DIR")]
    MissingOutDirForDependencies,
    /// Cached sidecars are validated before any generated path is joined.
    #[error("invalid cached OUT_DIR dependency metadata: {0}")]
    InvalidCachedOutDirDependencies(String),
    /// Current Cargo-authoritative dep-info no longer names the exact cached
    /// generation. Values remain redacted.
    #[error("cached observed inputs differ for descriptor unit {unit}")]
    CachedObservedInputsMismatch {
        /// Cached base unit.
        unit: DescriptorCargoUnitId,
    },
    /// Current generated source bytes diverged before final compilation.
    #[error("generated source changed between RSJS compilation passes: `{path}`")]
    OutDirDependencyHashMismatch {
        /// Exact final input path.
        path: PathBuf,
        /// Descriptor-time hash.
        expected: String,
        /// Current final hash.
        actual: String,
    },
    /// Final rustc observed a different generated-source set than descriptor
    /// rustc. This intentionally rejects final-only includes in v1.
    #[error("final rustc OUT_DIR dependency set differs from descriptor compilation (expected {expected:?}, observed {actual:?})")]
    FinalOutDirDependencySetMismatch {
        /// Descriptor-time exact set.
        expected: Vec<DescriptorOutDirDependency>,
        /// Final rustc exact set.
        actual: Vec<DescriptorOutDirDependency>,
    },
    /// Final rustc observed another exact non-OUT_DIR source set.
    #[error("final rustc source dependency set differs from descriptor compilation (expected {expected:?}, observed {actual:?})")]
    FinalSourceDependencySetMismatch {
        /// Descriptor-time exact set.
        expected: Vec<DescriptorSourceDependency>,
        /// Final rustc exact set.
        actual: Vec<DescriptorSourceDependency>,
    },
    /// Final rustc observed another redacted environment set.
    #[error("final rustc environment dependency set differs from descriptor compilation (expected {expected:?}, observed {actual:?})")]
    FinalEnvironmentDependencySetMismatch {
        /// Descriptor-time redacted set.
        expected: Vec<DescriptorEnvironmentDependency>,
        /// Final rustc redacted set.
        actual: Vec<DescriptorEnvironmentDependency>,
    },
    /// Cargo's build-script output declares the arbitrary rustc-env names.
    #[error(
        "failed to read Cargo build-script rustc-env declarations for OUT_DIR {out_dir:?}: {source}"
    )]
    ReadBuildScriptEnvironment {
        /// Cargo-provided output directory, when one was present.
        out_dir: Option<PathBuf>,
        /// Missing/malformed build-script output.
        #[source]
        source: io::Error,
    },
    /// Staging directory creation failed.
    #[error("failed to create descriptor staging directory `{path}`: {source}")]
    CreateStagingDirectory {
        /// Directory path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Successful rustc did not produce the explicitly requested object.
    #[error("rustc succeeded without producing exact descriptor object `{0}`")]
    MissingDescriptorObject(PathBuf),
    /// Unit sidecar serialization failed.
    #[error("failed to serialize descriptor unit sidecar: {0}")]
    SerializeUnitSidecar(serde_json::Error),
    /// Sidecar target has no valid parent.
    #[error("descriptor sidecar path has no parent: `{0}`")]
    SidecarWithoutParent(PathBuf),
    /// Sidecar path has no UTF-8 filename.
    #[error("descriptor sidecar has an invalid filename: `{0}`")]
    InvalidSidecarFileName(PathBuf),
    /// Atomic sidecar write failed.
    #[error("failed to atomically write descriptor sidecar `{path}`: {source}")]
    WriteSidecar {
        /// Output or temporary path.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Same content identity unexpectedly produced different sidecar bytes.
    #[error("descriptor unit sidecar conflicts with an existing file: `{0}`")]
    ConflictingUnitSidecar(PathBuf),
    /// A sidecar or orchestrator input contained a malformed unit identity.
    #[error("invalid descriptor Cargo-unit identity `{value}`: {reason}")]
    InvalidCargoUnitId {
        /// Rejected representation.
        value: String,
        /// Validation detail.
        reason: String,
    },
}

struct RustcWrapperDiagnostic {
    code: &'static str,
    message: String,
    notes: Vec<String>,
    help: Option<&'static str>,
}

impl RustcWrapperDiagnostic {
    fn for_error(error: &WrapperError) -> Self {
        match error {
            WrapperError::ObservedSourceHashMismatch { path, .. } => Self {
                code: "RSJS0001",
                message: "a source file changed while RSJS was linking".to_string(),
                notes: vec![
                    format!("changed file: `{}`", path.display()),
                    "RSJS stopped before combining stale descriptor output with current source"
                        .to_string(),
                ],
                help: Some(
                    "retry the command after the file stops changing; if this repeats, check for a formatter or generator that keeps rewriting it",
                ),
            },
            WrapperError::OutDirDependencyHashMismatch { path, .. } => Self {
                code: "RSJS0002",
                message: "generated source changed while RSJS was linking".to_string(),
                notes: vec![
                    format!("changed generated file: `{}`", path.display()),
                    "RSJS stopped before combining stale descriptor output with current generated source"
                        .to_string(),
                ],
                help: Some(
                    "retry the command; if this repeats, check the build script or generator that owns this file",
                ),
            },
            WrapperError::ObservedEnvironmentMismatch { name } => Self {
                code: "RSJS0003",
                message: "the build environment changed while RSJS was linking".to_string(),
                notes: vec![
                    format!("changed environment variable: `{name}`"),
                    "RSJS stopped before combining artifacts built with different environments"
                        .to_string(),
                ],
                help: Some("retry the command with a stable build environment"),
            },
            _ => Self {
                code: "RSJS0000",
                message: format!("RSJS compiler wrapper failed: {error}"),
                notes: Vec::new(),
                help: None,
            },
        }
    }

    fn rendered(&self) -> String {
        let mut rendered = format!("error[{}]: {}\n", self.code, self.message);
        for note in &self.notes {
            let _ = writeln!(rendered, "  = note: {note}");
        }
        if let Some(help) = self.help {
            let _ = writeln!(rendered, "  = help: {help}");
        }
        rendered
    }
}

/// Render a hidden wrapper failure using rustc's JSON diagnostic protocol.
///
/// Cargo recognizes this as a compiler error and therefore does not replace
/// the useful failure with its full internal wrapper/rustc command line.
pub(crate) fn rustc_wrapper_diagnostic_json(error: &WrapperError) -> String {
    let diagnostic = RustcWrapperDiagnostic::for_error(error);
    let mut children = diagnostic
        .notes
        .iter()
        .map(|message| {
            serde_json::json!({
                "message": message,
                "code": null,
                "level": "note",
                "spans": [],
                "children": [],
                "rendered": null,
            })
        })
        .collect::<Vec<_>>();
    if let Some(help) = diagnostic.help {
        children.push(serde_json::json!({
            "message": help,
            "code": null,
            "level": "help",
            "spans": [],
            "children": [],
            "rendered": null,
        }));
    }
    serde_json::json!({
        "$message_type": "diagnostic",
        "message": diagnostic.message,
        "code": {
            "code": diagnostic.code,
            "explanation": null,
        },
        "level": "error",
        "spans": [],
        "children": children,
        "rendered": diagnostic.rendered(),
    })
    .to_string()
}

/// Emit one Cargo-readable compiler diagnostic for a hidden wrapper failure.
pub(crate) fn emit_rustc_wrapper_diagnostic(error: &WrapperError) -> io::Result<()> {
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    writeln!(stderr, "{}", rustc_wrapper_diagnostic_json(error))
}

/// Cargo JSON protocol failure.
#[derive(Debug, Error)]
pub enum CargoMessageError {
    /// Stream line could not be read.
    #[error("failed to read Cargo JSON line {line}: {source}")]
    Read {
        /// One-based line number.
        line: usize,
        /// I/O error.
        #[source]
        source: io::Error,
    },
    /// Cargo emitted an empty stdout line in JSON mode.
    #[error("empty Cargo JSON message at line {line}")]
    EmptyLine {
        /// One-based line number.
        line: usize,
    },
    /// JSON syntax is malformed.
    #[error("malformed Cargo JSON at line {line}: {source}")]
    MalformedJson {
        /// One-based line number.
        line: usize,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Required `reason` discriminator is absent.
    #[error("Cargo JSON message at line {line} has no string `reason`")]
    MissingReason {
        /// One-based line number.
        line: usize,
    },
    /// A future reason must be reviewed before it is ignored.
    #[error("unsupported Cargo JSON reason `{reason}` at line {line}")]
    UnknownReason {
        /// One-based line number.
        line: usize,
        /// Cargo reason.
        reason: String,
    },
    /// A known non-artifact message omitted fields needed to trust the stream.
    #[error("malformed Cargo `{reason}` message at line {line}")]
    MalformedAuxiliaryMessage {
        /// One-based line number.
        line: usize,
        /// Cargo reason.
        reason: String,
    },
    /// Artifact message does not match Cargo's required schema.
    #[error("malformed Cargo compiler-artifact at line {line}: {source}")]
    MalformedCompilerArtifact {
        /// One-based line number.
        line: usize,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Semantically required artifact identity material was empty.
    #[error("incomplete Cargo compiler-artifact at line {line}")]
    IncompleteCompilerArtifact {
        /// One-based line number.
        line: usize,
    },
    /// Cargo repeated exactly the same artifact message.
    #[error(
        "duplicate Cargo compiler-artifact for `{package_id}` target `{target}` at line {line}"
    )]
    DuplicateCompilerArtifact {
        /// One-based line number.
        line: usize,
        /// Cargo package.
        package_id: String,
        /// Cargo target.
        target: String,
    },
    /// Terminal message lacks its success flag.
    #[error("malformed Cargo build-finished message at line {line}")]
    MalformedBuildFinished {
        /// One-based line number.
        line: usize,
    },
    /// Any content after the terminal record is suspicious.
    #[error("Cargo JSON message appeared after build-finished at line {line}")]
    MessageAfterBuildFinished {
        /// One-based line number.
        line: usize,
    },
    /// Cargo reported compilation failure.
    #[error("descriptor Cargo compilation failed")]
    CargoBuildFailed,
    /// Stream ended without a terminal record.
    #[error("Cargo JSON stream ended without build-finished")]
    MissingBuildFinished,
}

/// Failure to join Cargo-enumerated artifacts to wrapper units.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArtifactMatchError {
    /// Exact path membership is required.
    #[error("descriptor unit {0} has no required Cargo filename")]
    NoRequiredFilenames(DescriptorCargoUnitId),
    /// A unit selector was repeated.
    #[error("descriptor unit {0} has duplicate artifact expectations")]
    DuplicateUnitExpectation(DescriptorCargoUnitId),
    /// No message matched every selector field.
    #[error("no exact Cargo compiler-artifact matched descriptor unit {0}")]
    MissingArtifact(DescriptorCargoUnitId),
    /// More than one message matched every selector field.
    #[error("descriptor unit {unit} matched {matches} Cargo compiler-artifacts")]
    AmbiguousArtifact {
        /// Descriptor unit.
        unit: DescriptorCargoUnitId,
        /// Candidate count.
        matches: usize,
    },
    /// Two units attempted to claim one Cargo message.
    #[error("Cargo compiler-artifact `{artifact}` was claimed by multiple descriptor units")]
    ArtifactClaimedByMultipleUnits {
        /// Representative exact Cargo path.
        artifact: PathBuf,
    },
}

#[cfg(test)]
mod tests;
