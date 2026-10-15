// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use tempfile::TempDir;

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn fixture_request(temp: &TempDir) -> LinkedCargoRequest {
    LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo: OsString::from("cargo"),
            wrapper: temp.path().join("cargo-proa"),
            working_directory: temp.path().to_path_buf(),
            staging_directory: temp.path().join("generated/staging"),
            target_directory: temp.path().join("generated/target"),
            source_exclusion_directory: temp.path().join("generated"),
            cargo_args: vec![OsString::from("build")],
            environment: BTreeMap::new(),
        },
        root: DescriptorRootSelector {
            package_id: "path+file:///fixture#0.0.0".to_string(),
            target_name: "fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("phase-argument-fixture"),
        producer: ProducerVersion::new(0, 1, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("generated/plans"),
        final_target_root: temp.path().join("generated/final"),
        final_cargo_args: vec![OsString::from("build")],
        final_environment: BTreeMap::new(),
    }
}

#[test]
fn linked_pipeline_rejects_phase_environment_value_drift_before_cargo() {
    let temp = TempDir::new().unwrap();
    let mut descriptor_environment = BTreeMap::new();
    descriptor_environment.insert(OsString::from("APP_MODE"), OsString::from("descriptor"));
    let mut final_environment = BTreeMap::new();
    final_environment.insert(OsString::from("APP_MODE"), OsString::from("final"));
    let request = LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo: OsString::from("cargo"),
            wrapper: temp.path().join("cargo-proa"),
            working_directory: temp.path().to_path_buf(),
            staging_directory: temp.path().join("generated/staging"),
            target_directory: temp.path().join("generated/target"),
            source_exclusion_directory: temp.path().join("generated"),
            cargo_args: vec![OsString::from("build")],
            environment: descriptor_environment,
        },
        root: DescriptorRootSelector {
            package_id: "path+file:///fixture#0.0.0".to_string(),
            target_name: "fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("phase-environment-drift"),
        producer: ProducerVersion::new(0, 1, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("generated/plans"),
        final_target_root: temp.path().join("generated/final"),
        final_cargo_args: vec![OsString::from("build")],
        final_environment,
    };

    assert!(matches!(
        validate_request(&request),
        Err(LinkedCargoError::PhaseEnvironmentMismatch { names })
            if names == vec![OsString::from("APP_MODE")]
    ));
}

#[test]
fn linked_pipeline_rejects_divergent_target_selection_before_cargo() {
    let temp = TempDir::new().unwrap();
    let mut request = fixture_request(&temp);
    request.descriptor.cargo_args = ["build", "--bin", "a"]
        .map(OsString::from)
        .into_iter()
        .collect();
    request.final_cargo_args = ["build", "--bin", "b"]
        .map(OsString::from)
        .into_iter()
        .collect();

    assert!(matches!(
        validate_request(&request),
        Err(LinkedCargoError::PhaseCargoArgumentsMismatch { .. })
    ));
}

#[test]
fn linked_pipeline_rejects_feature_profile_and_target_drift_before_cargo() {
    let temp = TempDir::new().unwrap();
    for (descriptor_tail, final_tail) in [
        (vec!["--features", "alpha"], vec!["--features", "beta"]),
        (vec!["--profile", "dev"], vec!["--profile", "release"]),
        (
            vec!["--target", "x86_64-unknown-linux-gnu"],
            vec!["--target", "aarch64-unknown-linux-gnu"],
        ),
        (
            vec!["--config", "build.rustflags=[]"],
            vec!["--config", "build.rustflags=['--cfg=other']"],
        ),
    ] {
        let mut request = fixture_request(&temp);
        request.descriptor.cargo_args = std::iter::once(OsString::from("build"))
            .chain(descriptor_tail.into_iter().map(OsString::from))
            .collect();
        request.final_cargo_args = std::iter::once(OsString::from("build"))
            .chain(final_tail.into_iter().map(OsString::from))
            .collect();

        assert!(matches!(
            validate_request(&request),
            Err(LinkedCargoError::PhaseCargoArgumentsMismatch { .. })
        ));
    }
}

#[test]
fn linked_pipeline_accepts_only_the_lifecycle_phase_rewrites() {
    validate_phase_cargo_arguments(
        &["build", "-p", "app", "--bin", "server"].map(OsString::from),
        &["run", "-p", "app", "--bin", "server", "--", "--probe"].map(OsString::from),
    )
    .unwrap();
    validate_phase_cargo_arguments(
        &["test", "-p", "app", "--bin", "server", "--no-run"].map(OsString::from),
        &["test", "-p", "app", "--bin", "server", "--", "--nocapture"].map(OsString::from),
    )
    .unwrap();
    validate_phase_cargo_arguments(
        &["check", "-p", "app", "--bin", "server"].map(OsString::from),
        &["check", "-p", "app", "--bin", "server"].map(OsString::from),
    )
    .unwrap();
}

#[test]
fn linked_pipeline_rejects_overlapping_generated_namespaces_before_writing() {
    let temp = TempDir::new().unwrap();
    let mut request = fixture_request(&temp);
    request.plan_directory = request.descriptor.target_directory.join("plans");

    assert!(matches!(
        validate_request(&request),
        Err(LinkedCargoError::ManagedDirectoryOverlap {
            first_kind: "descriptor target",
            second_kind: "semantic plans",
            ..
        })
    ));
    assert!(!request.descriptor.target_directory.exists());
}

#[cfg(unix)]
#[test]
fn linked_pipeline_rejects_canonical_aliases_between_generated_namespaces() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let mut request = fixture_request(&temp);
    fs::create_dir_all(&request.plan_directory).unwrap();
    let aliased_target = request
        .descriptor
        .source_exclusion_directory
        .join("descriptor-target-alias");
    symlink(&request.plan_directory, &aliased_target).unwrap();
    request.descriptor.target_directory = aliased_target;

    assert!(matches!(
        validate_request(&request),
        Err(LinkedCargoError::DescriptorManagedPaths(_))
    ));
}

#[test]
fn linked_pipeline_rejects_cache_overlap_with_plan_or_final_before_writing() {
    let temp = TempDir::new().unwrap();
    let request = fixture_request(&temp);
    let managed = validate_request(&request).unwrap();

    assert!(matches!(
        validate_cache_against_linked_paths(&request.plan_directory, &managed),
        Err(LinkedCargoError::ManagedDirectoryOverlap {
            first_kind: "descriptor cache",
            second_kind: "semantic plans",
            ..
        })
    ));
    assert!(!request.plan_directory.exists());
}

#[test]
fn selector_rejects_non_root_roles_and_relative_artifacts() {
    let mut target_kinds = BTreeSet::new();
    target_kinds.insert("bin".to_string());
    let mut selector = DescriptorRootSelector {
        package_id: "path+file:///fixture#app@0.0.0".to_string(),
        target_name: "app".to_string(),
        target_kinds,
        role: ArtifactRole::RustLibrary,
        artifact_path: None,
    };
    assert!(matches!(
        validate_selector(&selector),
        Err(LinkedCargoError::NonRootRole(ArtifactRole::RustLibrary))
    ));

    selector.role = ArtifactRole::RootExecutable;
    selector.artifact_path = Some(PathBuf::from("target/app"));
    let artifact = CargoCompilerArtifact {
        package_id: selector.package_id.clone(),
        manifest_path: PathBuf::from("/fixture/Cargo.toml"),
        target: crate::build::CargoArtifactTarget {
            name: selector.target_name.clone(),
            kind: selector.target_kinds.clone(),
            crate_types: BTreeSet::from(["bin".to_string()]),
            src_path: PathBuf::from("/fixture/src/main.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([PathBuf::from("/fixture/target/app")]),
        executable: Some(PathBuf::from("/fixture/target/app")),
        fresh: false,
    };
    assert!(matches!(
        select_root_path(&artifact, &selector),
        Err(LinkedCargoError::RelativeRootArtifact(_))
    ));
}

#[test]
fn final_root_attestation_requires_the_descriptor_coordinate_and_real_file() {
    let temp = TempDir::new().unwrap();
    let target_directory = temp.path().join("generated/final-target");
    fs::create_dir_all(&target_directory).unwrap();
    let executable = target_directory.join("fixture");
    fs::write(&executable, b"binary").unwrap();

    let selector = DescriptorRootSelector {
        package_id: "path+file:///fixture#app@0.0.0".to_string(),
        target_name: "fixture".to_string(),
        target_kinds: BTreeSet::from(["bin".to_string()]),
        role: ArtifactRole::RootExecutable,
        artifact_path: None,
    };
    let descriptor_root = CargoCompilerArtifact {
        package_id: selector.package_id.clone(),
        manifest_path: temp.path().join("Cargo.toml"),
        target: crate::build::CargoArtifactTarget {
            name: selector.target_name.clone(),
            kind: selector.target_kinds.clone(),
            crate_types: BTreeSet::from(["bin".to_string()]),
            src_path: temp.path().join("src/main.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0"}),
        features: BTreeSet::from(["default".to_string()]),
        filenames: BTreeSet::from([temp.path().join("descriptor/fixture")]),
        executable: Some(temp.path().join("descriptor/fixture")),
        fresh: false,
    };
    let mut final_root = descriptor_root.clone();
    final_root.filenames = BTreeSet::from([executable.clone()]);
    final_root.executable = Some(executable);
    let plan_id = rsjs_ir::abi::PlanId::derive("final-root-attestation");
    let output = FinalCargoOutput {
        plan_id,
        final_unit_id: rsjs_ir::abi::FinalUnitId::for_plan(
            SemanticHash::derive("fixture-root"),
            plan_id,
        ),
        target_directory,
        unit_map_path: temp.path().join("generated/map.json"),
        unit_map_hash: "00".repeat(32),
        plan_file_hash: format!("sha256:{}", "11".repeat(32)),
        cargo_artifacts: vec![FinalCargoArtifactReport::from(&final_root)],
    };

    assert_eq!(
        attest_final_root(
            &output,
            std::slice::from_ref(&final_root),
            &selector,
            &descriptor_root,
        )
        .unwrap(),
        final_root
    );

    let mut mismatched_root = final_root;
    mismatched_root
        .features
        .insert("descriptor-final-drift".to_string());
    assert!(matches!(
        attest_final_root(
            &output,
            std::slice::from_ref(&mismatched_root),
            &selector,
            &descriptor_root,
        ),
        Err(LinkedCargoError::FinalRootCoordinateMismatch { .. })
    ));
}

#[cfg(unix)]
#[test]
fn a_mismatched_final_root_is_rejected_before_its_executable_can_run() {
    let temp = TempDir::new().unwrap();
    let target_directory = temp.path().join("generated/final-target");
    fs::create_dir_all(&target_directory).unwrap();
    let executable = target_directory.join("must-not-run.sh");
    let sentinel = temp.path().join("executed");
    write_executable(
        &executable,
        "#!/bin/sh\nprintf executed > \"$EXECUTION_SENTINEL\"\n",
    );

    let mut request = fixture_request(&temp);
    request.final_cargo_args = ["run", "--", "ignored"].map(OsString::from).to_vec();
    let sentinel_environment = (
        OsString::from("EXECUTION_SENTINEL"),
        sentinel.clone().into_os_string(),
    );
    request.descriptor.environment.insert(
        sentinel_environment.0.clone(),
        sentinel_environment.1.clone(),
    );
    request
        .final_environment
        .insert(sentinel_environment.0, sentinel_environment.1);
    let selector = request.root.clone();
    let descriptor_root = fixture_root_artifact(&selector, temp.path(), None);
    let mut final_root = descriptor_root.clone();
    final_root.features.insert("wrong-final-root".to_string());
    final_root.filenames = BTreeSet::from([executable.clone()]);
    final_root.executable = Some(executable);
    let output = fixture_final_output(&temp, target_directory, &final_root);

    assert!(matches!(
        attest_final_root(
            &output,
            std::slice::from_ref(&final_root),
            &selector,
            &descriptor_root,
        ),
        Err(LinkedCargoError::FinalRootCoordinateMismatch { .. })
    ));
    assert!(!sentinel.exists());
}

#[cfg(unix)]
#[test]
fn attested_execution_forwards_runtime_arguments_environment_and_nonzero_status() {
    let temp = TempDir::new().unwrap();
    let target_directory = temp.path().join("generated/final-target");
    fs::create_dir_all(&target_directory).unwrap();
    let executable = target_directory.join("attested.sh");
    let log = temp.path().join("execution.log");
    write_executable(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$FORWARDED_ENV\" \"$1\" \"$2\" > \"$EXECUTION_LOG\"\nexit \"${EXIT_CODE:-0}\"\n",
        );

    let mut request = fixture_request(&temp);
    request.final_cargo_args = ["run", "--", "first", "two words"]
        .map(OsString::from)
        .to_vec();
    let execution_environment = [
        (
            OsString::from("EXECUTION_LOG"),
            log.clone().into_os_string(),
        ),
        (OsString::from("FORWARDED_ENV"), OsString::from("present")),
    ];
    request
        .descriptor
        .environment
        .extend(execution_environment.clone());
    request.final_environment.extend(execution_environment);
    let selector = request.root.clone();
    let descriptor_root = fixture_root_artifact(&selector, temp.path(), None);
    let mut final_root = descriptor_root.clone();
    final_root.filenames = BTreeSet::from([executable.clone()]);
    final_root.executable = Some(executable.clone());
    let output = fixture_final_output(&temp, target_directory, &final_root);

    let attested = attest_final_root(
        &output,
        std::slice::from_ref(&final_root),
        &selector,
        &descriptor_root,
    )
    .unwrap();
    execute_requested_final_root(&request, &attested).unwrap();
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        format!(
            "{}\npresent\nfirst\ntwo words\n",
            fs::canonicalize(temp.path()).unwrap().display()
        )
    );

    request
        .final_environment
        .insert(OsString::from("EXIT_CODE"), OsString::from("23"));
    request
        .descriptor
        .environment
        .insert(OsString::from("EXIT_CODE"), OsString::from("23"));
    assert!(matches!(
        execute_requested_final_root(&request, &final_root),
        Err(LinkedCargoError::FinalRootExited { path, status })
            if path == executable && status.code() == Some(23)
    ));
}

fn fixture_root_artifact(
    selector: &DescriptorRootSelector,
    root: &Path,
    executable: Option<PathBuf>,
) -> CargoCompilerArtifact {
    let fallback = root.join("descriptor/fixture");
    CargoCompilerArtifact {
        package_id: selector.package_id.clone(),
        manifest_path: root.join("Cargo.toml"),
        target: crate::build::CargoArtifactTarget {
            name: selector.target_name.clone(),
            kind: selector.target_kinds.clone(),
            crate_types: BTreeSet::from(["bin".to_string()]),
            src_path: root.join("src/main.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0"}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([executable.clone().unwrap_or_else(|| fallback.clone())]),
        executable: Some(executable.unwrap_or(fallback)),
        fresh: false,
    }
}

fn fixture_final_output(
    temp: &TempDir,
    target_directory: PathBuf,
    root: &CargoCompilerArtifact,
) -> FinalCargoOutput {
    let plan_id = rsjs_ir::abi::PlanId::derive("execution-attestation");
    FinalCargoOutput {
        plan_id,
        final_unit_id: rsjs_ir::abi::FinalUnitId::for_plan(
            SemanticHash::derive("execution-root"),
            plan_id,
        ),
        target_directory,
        unit_map_path: temp.path().join("generated/map.json"),
        unit_map_hash: "00".repeat(32),
        plan_file_hash: format!("sha256:{}", "11".repeat(32)),
        cargo_artifacts: vec![FinalCargoArtifactReport::from(root)],
    }
}
