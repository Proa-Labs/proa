// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use tempfile::TempDir;

#[test]
fn wrapper_control_set_covers_every_phase_owned_variable() {
    for name in [
        WRAPPER_MODE_ENV,
        DESCRIPTOR_STAGING_DIR_ENV,
        DESCRIPTOR_TARGET_DIR_ENV,
        SOURCE_EXCLUSION_DIR_ENV,
        FINAL_TARGET_DIR_ENV,
        FINAL_UNIT_MAP_PATH_ENV,
        FINAL_UNIT_MAP_HASH_ENV,
        CARGO_UNIT_ID_ENV,
        FINAL_CARGO_UNIT_STATUS_ENV,
        DESCRIPTOR_PHASE_ENV,
        FINAL_PHASE_ENV,
        FINAL_PLAN_PATH_ENV,
        FINAL_PLAN_ID_ENV,
        FINAL_PLAN_FILE_HASH_ENV,
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        assert!(
            wrapper_control_environment(name),
            "phase-owned environment `{name}` must be scrubbed at every process boundary"
        );
    }
}

#[test]
fn source_hash_mismatch_renders_as_an_actionable_rustc_diagnostic() {
    let error = WrapperError::ObservedSourceHashMismatch {
        path: PathBuf::from("/workspace/scripts/install.ps1"),
        expected: "expected-hash".to_string(),
        actual: "actual-hash".to_string(),
    };

    let json = rustc_wrapper_diagnostic_json(&error);
    let diagnostic: Value = serde_json::from_str(&json).unwrap();

    assert_eq!(diagnostic["$message_type"], "diagnostic");
    assert_eq!(diagnostic["level"], "error");
    assert_eq!(diagnostic["code"]["code"], "RSJS0001");
    assert_eq!(
        diagnostic["message"],
        "a source file changed while RSJS was linking"
    );
    let rendered = diagnostic["rendered"].as_str().unwrap();
    assert!(rendered.contains("/workspace/scripts/install.ps1"));
    assert!(rendered.contains("retry the command"));
    assert!(rendered.contains("stale descriptor output"));
    assert!(!rendered.contains("expected-hash"));
    assert!(!rendered.contains("actual-hash"));
}

#[test]
fn rustc_probe_classifier_never_bypasses_compiling_print_requests() {
    assert!(is_rustc_probe(&[OsString::from("-vV")]));
    assert!(is_rustc_probe(&[
        OsString::from("-"),
        OsString::from("--crate-name"),
        OsString::from("___"),
        OsString::from("--crate-type=bin"),
        OsString::from("--print=file-names"),
        OsString::from("--print=sysroot"),
        OsString::from("--print=split-debuginfo"),
        OsString::from("--print=crate-name"),
        OsString::from("--print=cfg"),
    ]));
    assert!(!is_rustc_probe(&[
        OsString::from("--crate-name=fixture"),
        OsString::from("--crate-type=bin"),
        OsString::from("--emit=dep-info,link"),
        OsString::from("--print=link-args"),
        OsString::from("src/main.rs"),
    ]));
    assert!(!is_rustc_probe(&[
        OsString::from("--print=native-static-libs"),
        OsString::from("src/lib.rs"),
    ]));
}

#[test]
fn wrapper_argv_accepts_a_custom_rustc_but_rejects_cargo_wrapper_composition() {
    let direct = [
        OsString::from("cargo-proa"),
        OsString::from("/toolchain/custom-rustc"),
        OsString::from("-vV"),
    ];
    assert!(reject_composed_workspace_wrapper(&direct).is_ok());

    let composed = [
        OsString::from("cargo-proa"),
        OsString::from("/workspace/wrapper"),
        OsString::from("/toolchain/custom-rustc"),
        OsString::from("--crate-name"),
        OsString::from("fixture"),
    ];
    assert!(matches!(
        reject_composed_workspace_wrapper(&composed),
        Err(WrapperError::ComposedWorkspaceWrapper { wrapper, rustc })
            if wrapper == "/workspace/wrapper"
                && rustc == "/toolchain/custom-rustc"
    ));
}

#[cfg(unix)]
#[test]
fn command_scrub_removes_an_inherited_stale_plan_path() {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("test -z \"${RSJS_LINK_PLAN_PATH+x}\"")
        .env(FINAL_PLAN_PATH_ENV, "/stale/outer-plan.json");
    scrub_wrapper_control_environment(&mut command);
    assert!(command.status().unwrap().success());
}

#[cfg(unix)]
#[test]
fn final_child_receives_every_validated_phase_environment_value_after_scrub() {
    let values = BTreeMap::from([
        (FINAL_PHASE_ENV, OsString::from("1")),
        (FINAL_PLAN_PATH_ENV, OsString::from("/plans/exact.json")),
        (FINAL_PLAN_ID_ENV, OsString::from("11".repeat(32))),
        (
            FINAL_PLAN_FILE_HASH_ENV,
            OsString::from(format!("sha256:{}", "22".repeat(32))),
        ),
        (FINAL_UNIT_MAP_HASH_ENV, OsString::from("33".repeat(32))),
    ]);
    let script = FINAL_CHILD_ENVIRONMENT
        .iter()
        .map(|name| format!("printf '%s\\n' \"${{{name}}}\""))
        .collect::<Vec<_>>()
        .join("; ");
    let mut command = wrapped_rustc_command(OsStr::new("sh"));
    command.arg("-c").arg(script).envs(&values);
    let output = command.output().unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expected = FINAL_CHILD_ENVIRONMENT
        .iter()
        .map(|name| values.get(name).unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[cfg(unix)]
#[test]
fn target_lock_and_journal_namespaces_reject_symlink_redirection() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let target = root.join("generated/descriptor");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let redirected = root.join("redirected-locks");
    fs::create_dir_all(&redirected).unwrap();
    symlink(
        &redirected,
        target.parent().unwrap().join(".proa-rsjs-target-locks"),
    )
    .unwrap();
    assert!(matches!(
        acquire_descriptor_target_lease(&target),
        Err(DescriptorCargoRunError::UnsafeTargetLock(_))
    ));

    fs::remove_file(target.parent().unwrap().join(".proa-rsjs-target-locks")).unwrap();
    drop(acquire_descriptor_target_lease(&target).unwrap());
    let journal = descriptor_target_dirty_journal_path(&target).unwrap();
    let redirected_journal = root.join("redirected-journal.json");
    fs::write(&redirected_journal, b"{}\n").unwrap();
    symlink(&redirected_journal, &journal).unwrap();
    assert!(matches!(
        reject_cached_crash_journal(&target),
        Err(DescriptorCargoRunError::UnsafeTargetJournal(path)) if path == journal
    ));
}

fn managed_directory_request(root: &Path) -> DescriptorCargoRequest {
    let working = root.join("workspace");
    let generated = root.join("generated");
    fs::create_dir_all(&working).unwrap();
    fs::create_dir_all(&generated).unwrap();
    DescriptorCargoRequest {
        cargo: OsString::from("cargo"),
        wrapper: root.join("cargo-proa"),
        working_directory: working,
        staging_directory: generated.join("staging"),
        target_directory: generated.join("target"),
        source_exclusion_directory: generated,
        cargo_args: vec![OsString::from("check")],
        environment: BTreeMap::new(),
    }
}

#[test]
fn successful_descriptor_staging_cleanup_requires_runner_ownership() {
    let temp = TempDir::new().unwrap();
    let request = managed_directory_request(temp.path());
    fs::create_dir_all(&request.staging_directory).unwrap();
    write_ownership_marker(
        &request.staging_directory,
        DESCRIPTOR_STAGING_MARKER_FILE,
        DESCRIPTOR_STAGING_MARKER_BYTES,
    )
    .unwrap();
    fs::write(request.staging_directory.join("descriptor.o"), b"owned").unwrap();
    fs::create_dir_all(&request.target_directory).unwrap();
    fs::write(request.target_directory.join("keep"), b"target").unwrap();

    remove_successful_descriptor_staging(&request).unwrap();
    assert!(!request.staging_directory.exists());
    assert_eq!(
        fs::read(request.target_directory.join("keep")).unwrap(),
        b"target"
    );

    let mut unowned = managed_directory_request(temp.path());
    unowned.staging_directory = unowned.source_exclusion_directory.join("unowned-staging");
    fs::create_dir_all(&unowned.staging_directory).unwrap();
    fs::write(unowned.staging_directory.join("keep"), b"evidence").unwrap();

    assert!(matches!(
        remove_successful_descriptor_staging(&unowned),
        Err(DescriptorCargoRunError::UnownedCompletedStaging(_))
    ));
    assert_eq!(
        fs::read(unowned.staging_directory.join("keep")).unwrap(),
        b"evidence"
    );
}

#[test]
fn interrupted_staging_preparation_is_repeatable_but_fresh_claims_are_not() {
    let temp = TempDir::new().unwrap();
    let request = managed_directory_request(temp.path());

    let fresh = prepare_descriptor_staging_directory(&request, DescriptorStagingPreparation::Fresh)
        .unwrap();
    assert_eq!(fresh, request.staging_directory);
    fs::write(request.staging_directory.join("provisional"), b"evidence").unwrap();

    let first_attempt =
        prepare_descriptor_staging_directory(&request, DescriptorStagingPreparation::ResumeOwned)
            .unwrap();
    let second_attempt =
        prepare_descriptor_staging_directory(&request, DescriptorStagingPreparation::ResumeOwned)
            .unwrap();
    assert_ne!(first_attempt, second_attempt);
    assert!(first_attempt.is_dir());
    assert!(second_attempt.is_dir());
    assert_eq!(
        fs::read(request.staging_directory.join("provisional")).unwrap(),
        b"evidence"
    );
    assert!(matches!(
        prepare_descriptor_staging_directory(
            &request,
            DescriptorStagingPreparation::Fresh,
        ),
        Err(DescriptorCargoRunError::StagingAlreadyExists(path))
            if path == request.staging_directory
    ));
}

#[test]
fn interrupted_sidecar_temporary_name_is_strict() {
    let unit = "11".repeat(32);
    assert!(interrupted_sidecar_temporary(OsStr::new(&format!(
        ".{unit}.json.123.0.tmp"
    ))));
    assert!(!interrupted_sidecar_temporary(OsStr::new(&format!(
        "{unit}.json.123.0.tmp"
    ))));
    assert!(!interrupted_sidecar_temporary(OsStr::new(&format!(
        ".{unit}.json.pid.0.tmp"
    ))));
    assert!(!interrupted_sidecar_temporary(OsStr::new(
        ".not-a-unit.json.123.0.tmp"
    )));
    assert!(!interrupted_sidecar_temporary(OsStr::new(
        ".provisional.json.123.0.tmp.backup"
    )));
}

#[test]
fn descriptor_recovery_attempt_name_is_strict() {
    assert!(descriptor_recovery_attempt_name(OsStr::new(
        "attempt-123-0"
    )));
    assert!(!descriptor_recovery_attempt_name(OsStr::new(
        "attempt-pid-0"
    )));
    assert!(!descriptor_recovery_attempt_name(OsStr::new(
        "attempt-123-0-extra"
    )));
    assert!(!descriptor_recovery_attempt_name(OsStr::new("123-0")));
}

#[test]
fn descriptor_recovery_attempts_reject_unrecognized_entries() {
    let temp = TempDir::new().unwrap();
    let request = managed_directory_request(temp.path());
    prepare_descriptor_staging_directory(&request, DescriptorStagingPreparation::Fresh).unwrap();
    let attempts = request
        .staging_directory
        .join(DESCRIPTOR_RECOVERY_ATTEMPTS_DIRECTORY);
    fs::create_dir(&attempts).unwrap();
    fs::write(attempts.join("not-an-attempt"), b"untrusted").unwrap();

    assert!(matches!(
        prepare_descriptor_staging_directory(
            &request,
            DescriptorStagingPreparation::ResumeOwned,
        ),
        Err(DescriptorCargoRunError::UnexpectedRecoveryStagingEntry(path))
            if path == attempts.join("not-an-attempt")
    ));
}

#[test]
fn descriptor_managed_paths_reject_overlap_dot_components_and_source_exclusion() {
    let temp = TempDir::new().unwrap();
    let mut request = managed_directory_request(temp.path());
    let validated = validate_descriptor_managed_paths(&request).unwrap();
    assert!(!managed_paths_overlap(
        &validated.target,
        &validated.staging
    ));

    request.staging_directory = request.target_directory.join("nested");
    assert!(matches!(
        validate_descriptor_managed_paths(&request),
        Err(DescriptorCargoRunError::OverlappingTargetAndStagingDirectories { .. })
    ));

    request = managed_directory_request(temp.path());
    request.target_directory = request.source_exclusion_directory.join("./target");
    assert!(matches!(
        validate_descriptor_managed_paths(&request),
        Err(DescriptorCargoRunError::AmbiguousManagedDirectory(_))
    ));

    request = managed_directory_request(temp.path());
    request.source_exclusion_directory = temp.path().to_path_buf();
    assert!(matches!(
        validate_descriptor_managed_paths(&request),
        Err(DescriptorCargoRunError::UnsafeSourceExclusion { .. })
    ));
}

#[cfg(unix)]
#[test]
fn descriptor_managed_paths_reject_existing_symlink_descendants() {
    let temp = TempDir::new().unwrap();
    let mut request = managed_directory_request(temp.path());
    let real = request.source_exclusion_directory.join("real");
    fs::create_dir_all(&real).unwrap();
    let alias = request.source_exclusion_directory.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    request.target_directory = alias.join("target");
    assert!(matches!(
        validate_descriptor_managed_paths(&request),
        Err(DescriptorCargoRunError::UnsafeManagedDirectoryComponent { path, .. })
            if path == alias
    ));
}

fn final_coordinate(
    cargo_extra_filename: Option<&str>,
    cargo_primary_package: bool,
) -> FinalUnitCoordinate {
    FinalUnitCoordinate {
        cargo_manifest_path: PathBuf::from("/fixture/Cargo.toml"),
        cargo_package_name: "fixture".to_string(),
        cargo_package_version: "1.0.0".to_string(),
        cargo_profile: Some("debug".to_string()),
        cargo_profile_fingerprint: "11".repeat(32),
        cargo_primary_package,
        cargo_extra_filename: cargo_extra_filename.map(str::to_string),
        crate_name: "fixture".to_string(),
        crate_types: vec!["lib".to_string()],
        target: None,
        features: vec!["serde".to_string()],
        source_hash: "22".repeat(32),
        logical_environment_hash: "44".repeat(32),
        cfgs: vec!["feature=\"serde\"".to_string()],
    }
}

fn rustc_args(source: &Path, dependency: &Path) -> Vec<OsString> {
    vec![
        "--crate-name".into(),
        "fixture".into(),
        "--edition=2021".into(),
        "--crate-type".into(),
        "lib,rlib".into(),
        "--cfg".into(),
        "unix".into(),
        "--cfg=feature=\"alpha\"".into(),
        "--emit=link,metadata".into(),
        "-C".into(),
        "opt-level=2".into(),
        "--extern".into(),
        format!("dep={}", dependency.display()).into(),
        source.as_os_str().to_owned(),
    ]
}

fn fixture_inputs() -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("lib.rs");
    let dependency = temp.path().join("libdep.rlib");
    fs::write(&source, b"pub fn fixture() {}\n").unwrap();
    fs::write(&dependency, b"exact dependency bytes").unwrap();
    (temp, source, dependency)
}

fn write_package_source_closure_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let source = root.join("src/lib.rs");
    let nested = root.join("src/nested");
    let dependency = root.join("target/debug/deps/libdep.rlib");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(dependency.parent().unwrap()).unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("node_modules/cache")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        b"[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(&source, b"mod nested;\npub use nested::answer;\n").unwrap();
    fs::write(nested.join("mod.rs"), b"mod deep;\npub use deep::answer;\n").unwrap();
    fs::write(
        nested.join("deep.rs"),
        b"pub const fn answer() -> u32 { 42 }\n",
    )
    .unwrap();
    fs::write(&dependency, b"same dependency bytes").unwrap();
    fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
    fs::write(root.join("node_modules/cache/metadata"), b"generated\n").unwrap();
    (source, dependency)
}

#[test]
fn cargo_unit_id_hex_parser_is_exact_and_strongly_typed() {
    let encoded = "000102030405060708090a0b0c0d0e0f\
                       101112131415161718191a1b1c1d1e1f";
    let unit = DescriptorCargoUnitId::from_hex(encoded).unwrap();
    assert_eq!(unit.as_bytes(), &core::array::from_fn(|index| index as u8));
    assert_eq!(unit.as_abi_id().as_bytes(), unit.as_bytes());
    assert!(DescriptorCargoUnitId::from_hex(&"A0".repeat(32)).is_err());
    assert!(DescriptorCargoUnitId::from_hex("00").is_err());
}

#[test]
fn final_plan_rustc_arguments_are_exact_and_ordered() {
    let plan_id = "11".repeat(32);
    let plan_file_hash = format!("sha256:{}", "22".repeat(32));
    let unit_map_hash = "33".repeat(32);

    assert_eq!(
        final_plan_rustc_args(&plan_id, &plan_file_hash, &unit_map_hash),
        vec![
            OsString::from("--cfg"),
            OsString::from("rsjs_final"),
            OsString::from("--cfg"),
            OsString::from(format!(
                "rsjs_final_wrapper_policy=\"{FINAL_WRAPPER_POLICY_VERSION}\""
            )),
            OsString::from("--check-cfg"),
            OsString::from("cfg(rsjs_final)"),
            OsString::from("--check-cfg"),
            OsString::from(format!(
                "cfg(rsjs_final_wrapper_policy,values(\"{FINAL_WRAPPER_POLICY_VERSION}\"))"
            )),
        ]
    );
}

#[test]
fn final_coordinate_uses_only_cargo_owned_cfgs() {
    let cargo_cfgs = BTreeSet::from(["feature=\"serde\"".to_string()]);
    assert_eq!(
        validate_cargo_owned_final_coordinate_cfgs(cargo_cfgs.clone()).unwrap(),
        cargo_cfgs
    );
    assert!(matches!(
        validate_cargo_owned_final_coordinate_cfgs(BTreeSet::from([
            "feature=\"serde\"".to_string(),
            "rsjs_final".to_string(),
        ])),
        Err(WrapperError::UnexpectedFinalPlanCfg(cfg)) if cfg == "rsjs_final"
    ));
    assert!(matches!(
        validate_cargo_owned_final_coordinate_cfgs(BTreeSet::from([
            "feature=\"serde\"".to_string(),
            format!(
                "rsjs_final_wrapper_policy=\"{FINAL_WRAPPER_POLICY_VERSION}\""
            ),
        ])),
        Err(WrapperError::UnexpectedFinalPlanCfg(cfg))
            if cfg.starts_with("rsjs_final_wrapper_policy")
    ));
}

#[test]
fn final_unit_lookup_requires_the_exact_cargo_suffix() {
    let first = DescriptorCargoUnitId([0x44; 32]);
    let second = DescriptorCargoUnitId([0x55; 32]);
    let map = LoadedFinalUnitMap {
        entries: BTreeMap::from([
            (
                final_coordinate(Some("-first"), false),
                LoadedFinalUnit {
                    cargo_unit_id: first,
                    out_dir_dependencies: Vec::new(),
                    source_dependencies: Vec::new(),
                    environment_dependencies: Vec::new(),
                },
            ),
            (
                final_coordinate(Some("-second"), false),
                LoadedFinalUnit {
                    cargo_unit_id: second,
                    out_dir_dependencies: Vec::new(),
                    source_dependencies: Vec::new(),
                    environment_dependencies: Vec::new(),
                },
            ),
        ]),
    };

    assert_eq!(
        map.resolve(Some(&final_coordinate(Some("-first"), false)))
            .map(|entry| entry.cargo_unit_id),
        Some(first)
    );
    assert_eq!(
        map.resolve(Some(&final_coordinate(Some("-second"), false)))
            .map(|entry| entry.cargo_unit_id),
        Some(second)
    );
    assert!(map
        .resolve(Some(&final_coordinate(Some("-stale"), false)))
        .is_none());
    assert!(map.resolve(Some(&final_coordinate(None, true))).is_none());
    assert!(map.resolve(None).is_none());
}

#[test]
fn normalized_flag_spelling_and_order_hash_identically() {
    let (_temp, source, dependency) = fixture_inputs();
    let first = rustc_args(&source, &dependency);
    let second = vec![
        "--extern".into(),
        format!("dep={}", dependency.display()).into(),
        "--cfg=feature=\"alpha\"".into(),
        "--crate-type=rlib,lib".into(),
        "-Copt-level=2".into(),
        "--emit".into(),
        "metadata,link".into(),
        "--edition".into(),
        "2021".into(),
        "--cfg=unix".into(),
        "--crate-name=fixture".into(),
        source.as_os_str().to_owned(),
    ];

    let environment = BTreeMap::new();
    let first =
        normalize_rustc_invocation(&first, &environment, b"rustc fixture identity").unwrap();
    let second =
        normalize_rustc_invocation(&second, &environment, b"rustc fixture identity").unwrap();
    assert_eq!(first.cargo_unit_id(), second.cargo_unit_id());
}

#[test]
fn dependency_paths_are_join_coordinates_not_unit_identity() {
    let (temp, source, first_dependency) = fixture_inputs();
    let second_dependency = temp.path().join("other/libdep.rlib");
    fs::create_dir_all(second_dependency.parent().unwrap()).unwrap();
    fs::copy(&first_dependency, &second_dependency).unwrap();

    let first = normalize_rustc_invocation(
        &rustc_args(&source, &first_dependency),
        &BTreeMap::new(),
        b"rustc fixture identity",
    )
    .unwrap();
    let second = normalize_rustc_invocation(
        &rustc_args(&source, &second_dependency),
        &BTreeMap::new(),
        b"rustc fixture identity",
    )
    .unwrap();

    assert_ne!(first.dependency_paths, second.dependency_paths);
    assert_eq!(first.dependencies, second.dependencies);
    assert_eq!(first.cargo_unit_id(), second.cargo_unit_id());
}

#[test]
fn test_harness_without_explicit_crate_type_normalizes_as_bin() {
    let (_temp, source, dependency) = fixture_inputs();
    let arguments = vec![
        "--crate-name=fixture_test".into(),
        "--edition=2021".into(),
        "--test".into(),
        "--emit=link".into(),
        "--extern".into(),
        format!("dep={}", dependency.display()).into(),
        source.as_os_str().to_owned(),
    ];

    let normalized =
        normalize_rustc_invocation(&arguments, &BTreeMap::new(), b"rustc fixture identity")
            .unwrap();
    assert_eq!(normalized.crate_types, BTreeSet::from(["bin".to_string()]));
    assert!(normalized.semantic_options.contains_key("--test"));
}

#[test]
fn cfg_feature_and_dependency_content_change_unit_identity() {
    let (_temp, source, dependency) = fixture_inputs();
    let environment = BTreeMap::new();
    let baseline = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment,
        b"rustc fixture identity",
    )
    .unwrap()
    .cargo_unit_id();

    let mut cfg_args = rustc_args(&source, &dependency);
    cfg_args.push("--cfg=custom_cfg".into());
    let cfg = normalize_rustc_invocation(&cfg_args, &environment, b"rustc fixture identity")
        .unwrap()
        .cargo_unit_id();

    let mut feature_environment = BTreeMap::new();
    feature_environment.insert("CARGO_FEATURE_BETA".to_string(), "1".to_string());
    let feature = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &feature_environment,
        b"rustc fixture identity",
    )
    .unwrap()
    .cargo_unit_id();

    fs::write(&dependency, b"changed dependency bytes").unwrap();
    let dependency_changed = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment,
        b"rustc fixture identity",
    )
    .unwrap()
    .cargo_unit_id();

    assert_ne!(baseline, cfg);
    assert_ne!(baseline, feature);
    assert_ne!(baseline, dependency_changed);
}

#[test]
fn base_identity_excludes_transport_and_out_dir_bytes_but_binds_build_script_env() {
    let (temp, source, dependency) = fixture_inputs();
    let first_out = temp.path().join("first-build/out");
    let second_out = temp.path().join("second-build/out");
    fs::create_dir_all(&first_out).unwrap();
    fs::create_dir_all(&second_out).unwrap();
    fs::write(
        first_out.join("generated.rs"),
        b"pub const VALUE: u8 = 1;\n",
    )
    .unwrap();
    fs::write(
        second_out.join("generated.rs"),
        b"pub const VALUE: u8 = 1;\n",
    )
    .unwrap();
    fs::write(
        first_out.parent().unwrap().join("output"),
        b"cargo::rustc-env=BUILD_MODE=stable\n",
    )
    .unwrap();
    fs::write(
        second_out.parent().unwrap().join("output"),
        b"cargo::rustc-env=BUILD_MODE=stable\n",
    )
    .unwrap();

    let environment = |out_dir: &Path, target_tmp: &str, ambient: &str, build_mode: &str| {
        BTreeMap::from([
            ("OUT_DIR".to_string(), out_dir.display().to_string()),
            ("BUILD_MODE".to_string(), build_mode.to_string()),
            ("CARGO_TARGET_TMPDIR".to_string(), target_tmp.to_string()),
            (
                "CARGO_BIN_EXE_fixture".to_string(),
                format!("{target_tmp}/fixture"),
            ),
            ("UNRELATED_AMBIENT".to_string(), ambient.to_string()),
            ("PROFILE".to_string(), "debug".to_string()),
        ])
    };
    let first = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment(&first_out, "/target/one", "host-a", "stable"),
        b"same toolchain",
    )
    .unwrap();
    let relocated = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment(&second_out, "/target/two", "host-b", "stable"),
        b"same toolchain",
    )
    .unwrap();
    assert_eq!(first.cargo_unit_id(), relocated.cargo_unit_id());
    assert_eq!(
        first.logical_environment_hash(),
        relocated.logical_environment_hash()
    );

    let changed_build_env = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment(&second_out, "/target/two", "host-b", "changed"),
        b"same toolchain",
    )
    .unwrap();
    assert_ne!(first.cargo_unit_id(), changed_build_env.cargo_unit_id());

    fs::write(
        second_out.join("generated.rs"),
        b"pub const VALUE: u8 = 2;\n",
    )
    .unwrap();
    let changed_out_dir = normalize_rustc_invocation(
        &rustc_args(&source, &dependency),
        &environment(&second_out, "/target/two", "host-b", "stable"),
        b"same toolchain",
    )
    .unwrap();
    assert_eq!(first.cargo_unit_id(), changed_out_dir.cargo_unit_id());
    assert_eq!(
        first.logical_environment_hash(),
        changed_out_dir.logical_environment_hash()
    );
}

#[test]
fn identical_inputs_are_independent_of_host_paths() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let first_source = first.path().join("src/lib.rs");
    let second_source = second.path().join("different/lib.rs");
    let first_dependency = first.path().join("target/a/libdep.rlib");
    let second_dependency = second.path().join("elsewhere/libdep.rlib");
    fs::create_dir_all(first_source.parent().unwrap()).unwrap();
    fs::create_dir_all(second_source.parent().unwrap()).unwrap();
    fs::create_dir_all(first_dependency.parent().unwrap()).unwrap();
    fs::create_dir_all(second_dependency.parent().unwrap()).unwrap();
    fs::write(&first_source, b"pub fn same() {}\n").unwrap();
    fs::write(&second_source, b"pub fn same() {}\n").unwrap();
    fs::write(&first_dependency, b"same dependency").unwrap();
    fs::write(&second_dependency, b"same dependency").unwrap();

    let first = normalize_rustc_invocation(
        &rustc_args(&first_source, &first_dependency),
        &BTreeMap::new(),
        b"same toolchain",
    )
    .unwrap();
    let second = normalize_rustc_invocation(
        &rustc_args(&second_source, &second_dependency),
        &BTreeMap::new(),
        b"same toolchain",
    )
    .unwrap();
    assert_eq!(first.cargo_unit_id(), second.cargo_unit_id());
}

#[test]
fn package_source_closure_is_host_independent_and_tracks_nested_modules() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let (first_source, first_dependency) = write_package_source_closure_fixture(first.path());
    let (second_source, second_dependency) = write_package_source_closure_fixture(second.path());
    let environment = BTreeMap::new();

    let first_input = normalize_rustc_invocation_with_package_root(
        &rustc_args(&first_source, &first_dependency),
        &environment,
        b"same toolchain",
        Some(first.path()),
        Some(&first.path().join("target")),
    )
    .unwrap();
    let second_input = normalize_rustc_invocation_with_package_root(
        &rustc_args(&second_source, &second_dependency),
        &environment,
        b"same toolchain",
        Some(second.path()),
        Some(&second.path().join("target")),
    )
    .unwrap();
    assert_eq!(
        first_input.package_source_closure_hash,
        second_input.package_source_closure_hash
    );
    assert_eq!(first_input.cargo_unit_id(), second_input.cargo_unit_id());

    fs::write(second.path().join(".git/HEAD"), b"different host ref\n").unwrap();
    fs::write(
        second.path().join("target/debug/generated.rs"),
        b"different generated output\n",
    )
    .unwrap();
    fs::write(
        second.path().join("node_modules/cache/metadata"),
        b"different dependency cache\n",
    )
    .unwrap();
    let excluded_files_changed = normalize_rustc_invocation_with_package_root(
        &rustc_args(&second_source, &second_dependency),
        &environment,
        b"same toolchain",
        Some(second.path()),
        Some(&second.path().join("target")),
    )
    .unwrap();
    assert_eq!(
        first_input.package_source_closure_hash,
        excluded_files_changed.package_source_closure_hash
    );
    assert_eq!(
        first_input.cargo_unit_id(),
        excluded_files_changed.cargo_unit_id()
    );

    fs::write(
        second.path().join("src/nested/deep.rs"),
        b"pub const fn answer() -> u32 { 43 }\n",
    )
    .unwrap();
    let nested_module_changed = normalize_rustc_invocation_with_package_root(
        &rustc_args(&second_source, &second_dependency),
        &environment,
        b"same toolchain",
        Some(second.path()),
        Some(&second.path().join("target")),
    )
    .unwrap();
    assert_ne!(
        first_input.package_source_closure_hash,
        nested_module_changed.package_source_closure_hash
    );
    assert_ne!(
        first_input.cargo_unit_id(),
        nested_module_changed.cargo_unit_id()
    );
}

#[test]
fn package_source_closure_excludes_only_conventional_targets_not_marker_subtrees() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src/target")).unwrap();
    fs::create_dir_all(root.join("src/node_modules")).unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::create_dir_all(root.join("node_modules/cache")).unwrap();
    fs::create_dir_all(root.join("macro-output/tests/trybuild/debug")).unwrap();
    fs::write(root.join("src/lib.rs"), b"pub mod target;\n").unwrap();
    fs::write(
        root.join("src/target/mod.rs"),
        b"pub const VALUE: u8 = 1;\n",
    )
    .unwrap();
    fs::write(root.join("src/node_modules/fixture.txt"), b"source asset\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("fixture.txt", root.join("src/node_modules/tool")).unwrap();
    fs::write(root.join("target/debug/generated.rs"), b"generated one\n").unwrap();
    fs::write(root.join("node_modules/cache/metadata"), b"cache one\n").unwrap();
    fs::write(
        root.join("macro-output/tests/trybuild/.rustc_info.json"),
        b"{}\n",
    )
    .unwrap();
    fs::write(
        root.join("macro-output/tests/trybuild/debug/fixture"),
        b"large generated artifact one\n",
    )
    .unwrap();

    let baseline = hash_package_source_closure(root, Some(&root.join("target"))).unwrap();

    fs::write(root.join("target/debug/generated.rs"), b"generated two\n").unwrap();
    fs::write(root.join("node_modules/cache/metadata"), b"cache two\n").unwrap();
    fs::write(
        root.join("macro-output/tests/trybuild/debug/fixture"),
        b"large generated artifact two\n",
    )
    .unwrap();
    let marker_subtree_changed =
        hash_package_source_closure(root, Some(&root.join("target"))).unwrap();
    assert_ne!(baseline, marker_subtree_changed);

    let conventional_target_baseline = hash_package_source_closure(root, None).unwrap();
    fs::write(root.join("target/debug/generated.rs"), b"generated three\n").unwrap();
    assert_eq!(
        conventional_target_baseline,
        hash_package_source_closure(root, None).unwrap()
    );

    fs::write(
        root.join("src/target/mod.rs"),
        b"pub const VALUE: u8 = 2;\n",
    )
    .unwrap();
    let nested_target_changed =
        hash_package_source_closure(root, Some(&root.join("target"))).unwrap();
    assert_ne!(marker_subtree_changed, nested_target_changed);

    fs::write(
        root.join("src/node_modules/fixture.txt"),
        b"changed source asset\n",
    )
    .unwrap();
    assert_eq!(
        nested_target_changed,
        hash_package_source_closure(root, Some(&root.join("target"))).unwrap()
    );
}

#[test]
fn package_source_closure_excludes_only_the_exact_custom_generated_subtree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".build/generated")).unwrap();
    fs::create_dir_all(root.join("other-build")).unwrap();
    fs::write(root.join("src/lib.rs"), b"pub fn value() {}\n").unwrap();
    fs::write(root.join(".build/generated/output"), b"first\n").unwrap();
    fs::write(root.join("other-build/input"), b"first\n").unwrap();
    let exclusion = root.join(".build");
    let baseline = hash_package_source_closure(root, Some(&exclusion)).unwrap();

    fs::write(root.join(".build/generated/output"), b"second\n").unwrap();
    assert_eq!(
        baseline,
        hash_package_source_closure(root, Some(&exclusion)).unwrap()
    );
    fs::write(root.join("other-build/input"), b"second\n").unwrap();
    assert_ne!(
        baseline,
        hash_package_source_closure(root, Some(&exclusion)).unwrap()
    );
}

#[cfg(unix)]
fn write_dep_info_fixture(path: &Path, dependencies: &[&Path]) {
    let encoded = dependencies
        .iter()
        .map(|dependency| dependency.display().to_string().replace(' ', "\\ "))
        .collect::<Vec<_>>();
    let mut body = format!("{}: {}\n\n", path.display(), encoded.join(" "));
    for dependency in encoded {
        body.push_str(&dependency);
        body.push_str(":\n");
    }
    fs::write(path, body).unwrap();
}

#[cfg(unix)]
#[test]
fn dep_info_tracks_only_consumed_out_dir_files_and_rejects_symlinks() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let out = target.join("debug/build/fixture/out");
    fs::create_dir_all(&out).unwrap();
    let generated = out.join("generated.rs");
    let obsolete = out.join("obsolete.rs");
    let outside = temp.path().join("outside.rs");
    fs::write(&generated, b"pub const VALUE: u8 = 1;\n").unwrap();
    fs::write(&obsolete, b"stale and unreferenced\n").unwrap();
    fs::write(&outside, b"outside\n").unwrap();
    let dep_info = target.join("debug/deps/fixture.d");
    fs::create_dir_all(dep_info.parent().unwrap()).unwrap();
    write_dep_info_fixture(&dep_info, &[&outside, &generated]);
    let observed = observe_rustc_inputs(
        &dep_info,
        temp.path(),
        Some(&out),
        Some("debug/build/fixture/out"),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(observed.out_dir_dependencies.len(), 1);
    assert_eq!(
        observed.out_dir_dependencies[0].relative_path,
        "generated.rs"
    );
    assert_eq!(observed.source_dependencies.len(), 1);
    assert_eq!(
        observed.source_dependencies[0].absolute_path,
        outside.canonicalize().unwrap()
    );

    let linked_outside = out.join("linked-outside.rs");
    std::os::unix::fs::symlink(&outside, &linked_outside).unwrap();
    write_dep_info_fixture(&dep_info, &[&linked_outside]);
    assert!(matches!(
        observe_rustc_inputs(
            &dep_info,
            temp.path(),
            Some(&out),
            Some("debug/build/fixture/out"),
            &BTreeMap::new(),
        ),
        Err(WrapperError::UnsafeOutDirDependency(path)) if path == linked_outside
    ));
    fs::remove_file(&linked_outside).unwrap();

    let in_tree_target = out.join("nested/target.rs");
    fs::create_dir_all(in_tree_target.parent().unwrap()).unwrap();
    fs::write(&in_tree_target, b"in tree\n").unwrap();
    let linked_in_tree = out.join("linked-in-tree.rs");
    std::os::unix::fs::symlink(&in_tree_target, &linked_in_tree).unwrap();
    write_dep_info_fixture(&dep_info, &[&linked_in_tree]);
    assert!(matches!(
        observe_rustc_inputs(
            &dep_info,
            temp.path(),
            Some(&out),
            Some("debug/build/fixture/out"),
            &BTreeMap::new(),
        ),
        Err(WrapperError::UnsafeOutDirDependency(path)) if path == linked_in_tree
    ));
}

#[cfg(unix)]
#[test]
fn dep_info_coalesces_identical_canonical_source_dependencies() {
    let temp = TempDir::new().unwrap();
    let docs = temp.path().join("crate/src/docs");
    let handler = temp.path().join("crate/src/handler");
    fs::create_dir_all(&docs).unwrap();
    fs::create_dir_all(&handler).unwrap();
    let source = docs.join("handlers_intro.md");
    fs::write(&source, b"shared documentation\n").unwrap();
    let lexical_alias = handler.join("../docs/handlers_intro.md");
    let dep_info = temp.path().join("target/debug/deps/fixture.d");
    fs::create_dir_all(dep_info.parent().unwrap()).unwrap();
    write_dep_info_fixture(&dep_info, &[&source, &lexical_alias]);

    let observed =
        observe_rustc_inputs(&dep_info, temp.path(), None, None, &BTreeMap::new()).unwrap();

    assert_eq!(
        observed.source_dependencies,
        vec![DescriptorSourceDependency {
            absolute_path: source.canonicalize().unwrap(),
            content_hash: hex_digest(
                &hash_file(&source, OBSERVED_SOURCE_FILE_HASH_DOMAIN).unwrap()
            ),
        }]
    );

    let canonical = source.canonicalize().unwrap();
    let mut dependencies = BTreeMap::new();
    record_observed_source_dependency(&mut dependencies, canonical.clone(), "first".to_string())
        .unwrap();
    record_observed_source_dependency(&mut dependencies, canonical.clone(), "first".to_string())
        .unwrap();
    assert!(matches!(
        record_observed_source_dependency(
            &mut dependencies,
            canonical.clone(),
            "second".to_string(),
        ),
        Err(WrapperError::ConflictingObservedSource(path)) if path == canonical
    ));
}

#[test]
fn dep_info_parser_handles_rustc_escaped_spaces_and_dollar_signs() {
    let dependencies = parse_rustc_dep_info(
            b"target: file.d: /tmp/generated\\ file.rs /tmp/dollar$$name.rs\n\n/tmp/generated\\ file.rs:\n/tmp/dollar$$name.rs:\n",
        )
        .unwrap()
        .source_paths;

    assert_eq!(
        dependencies,
        vec![
            PathBuf::from("/tmp/generated file.rs"),
            PathBuf::from("/tmp/dollar$$name.rs"),
        ]
    );
}

#[test]
fn dep_info_parser_preserves_windows_drives_and_backslashes() {
    let dependencies = parse_rustc_dep_info(
            br"C:\target\debug\crate.d: C:\workspace\src\lib.rs C:\target\build\pkg\out\generated\ file.rs

C:\workspace\src\lib.rs:
C:\target\build\pkg\out\generated\ file.rs:
",
        )
        .unwrap()
        .source_paths;

    assert_eq!(
        dependencies,
        vec![
            PathBuf::from(r"C:\workspace\src\lib.rs"),
            PathBuf::from(r"C:\target\build\pkg\out\generated file.rs"),
        ]
    );
}

#[test]
fn dep_info_parser_rejects_ambiguous_external_whitespace() {
    for malformed in [
        b"target: /tmp/foo\t /out/generated.rs\n\n/tmp/foo\t:\n/out/generated.rs:\n".as_slice(),
        b"target: /tmp/foo\nbar.rs /out/generated.rs\n\n/tmp/foo\nbar.rs:\n/out/generated.rs:\n"
            .as_slice(),
    ] {
        assert!(parse_rustc_dep_info(malformed).is_err());
    }
}

#[test]
fn dep_info_parser_preserves_trailing_space_and_backslash_paths() {
    let dependencies = parse_rustc_dep_info(
        b"target: /tmp/trailing\\  /tmp/back\\\n\n/tmp/trailing\\ :\n/tmp/back\\:\n",
    )
    .unwrap()
    .source_paths;

    assert_eq!(
        dependencies,
        vec![
            PathBuf::from("/tmp/trailing "),
            PathBuf::from("/tmp/back\\")
        ]
    );
}

#[test]
fn dep_info_parser_preserves_environment_presence_and_rustc_escapes() {
    let parsed = parse_rustc_dep_info(
            b"target: /tmp/lib.rs\n\n/tmp/lib.rs:\n# env-dep:ABSENT\n# env-dep:EMPTY=\n# env-dep:ESCAPED=one\\\\two\\nthree\\rfour\n",
        )
        .unwrap();
    let environment = parsed
        .environment
        .into_iter()
        .map(|dependency| (dependency.name, dependency.value))
        .collect::<Vec<_>>();
    assert_eq!(
        environment,
        vec![
            ("ABSENT".to_string(), None),
            ("EMPTY".to_string(), Some(String::new())),
            (
                "ESCAPED".to_string(),
                Some("one\\two\nthree\rfour".to_string())
            ),
        ]
    );
}

#[test]
fn dep_info_parser_rejects_disagreeing_outputs_and_duplicate_metadata() {
    assert!(
        parse_rustc_dep_info(b"first: /tmp/one.rs\nsecond: /tmp/two.rs\n\n/tmp/one.rs:\n").is_err()
    );
    assert!(parse_rustc_dep_info(
        b"target: /tmp/lib.rs\n\n/tmp/lib.rs:\n# env-dep:VALUE=one\n# env-dep:VALUE=two\n"
    )
    .is_err());
}

#[test]
fn observed_environment_is_redacted_phase_invariant_and_presence_exact() {
    let secret = "a-secret-that-must-never-reach-json-or-errors";
    let phase = BTreeMap::from([(
        CARGO_UNIT_ID_ENV.to_string(),
        Some("descriptor-unit".to_string()),
    )]);
    let normalized = normalize_environment_dependencies(
        vec![
            ParsedEnvironmentDependency {
                name: "ABSENT".to_string(),
                value: None,
            },
            ParsedEnvironmentDependency {
                name: "EMPTY".to_string(),
                value: Some(String::new()),
            },
            ParsedEnvironmentDependency {
                name: "SECRET".to_string(),
                value: Some(secret.to_string()),
            },
            ParsedEnvironmentDependency {
                name: CARGO_UNIT_ID_ENV.to_string(),
                value: Some("descriptor-unit".to_string()),
            },
        ],
        None,
        None,
        &phase,
    )
    .unwrap();
    let serialized = serde_json::to_string(&normalized).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains(CARGO_UNIT_ID_ENV));
    assert_ne!(normalized[0].value_hash, normalized[1].value_hash);

    let error = normalize_environment_dependencies(
        vec![ParsedEnvironmentDependency {
            name: CARGO_UNIT_ID_ENV.to_string(),
            value: Some(secret.to_string()),
        }],
        None,
        None,
        &phase,
    )
    .unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(matches!(
        error,
        WrapperError::PhaseOwnedEnvironmentMismatch { name }
            if name == CARGO_UNIT_ID_ENV
    ));
}

#[test]
fn out_dir_environment_hash_uses_the_portable_target_coordinate() {
    let first = PathBuf::from("/descriptor-target/debug/build/pkg/out");
    let second = PathBuf::from("/final-target/debug/build/pkg/out");
    let observe = |path: &Path| {
        normalize_environment_dependencies(
            vec![ParsedEnvironmentDependency {
                name: "OUT_DIR".to_string(),
                value: Some(path.display().to_string()),
            }],
            Some(path),
            Some("debug/build/pkg/out"),
            &BTreeMap::new(),
        )
        .unwrap()
    };
    assert_eq!(observe(&first), observe(&second));
}

#[test]
fn exact_dep_info_pipeline_rejects_path_remapping() {
    let (_temp, source, dependency) = fixture_inputs();
    for (suffix, expected) in [
        (
            vec![OsString::from("--remap-path-prefix=/host=/logical")],
            "--remap-path-prefix",
        ),
        (
            vec![OsString::from("--remap-path-scope=object")],
            "--remap-path-scope",
        ),
        (
            vec![OsString::from("-Zremap-cwd-prefix=/logical")],
            "-Zremap-cwd-prefix",
        ),
        (
            vec![
                OsString::from("-Z"),
                OsString::from("remap-path-scope=object"),
            ],
            "-Zremap-path-scope",
        ),
    ] {
        let mut args = rustc_args(&source, &dependency);
        args.extend(suffix);
        assert!(matches!(
            normalize_rustc_invocation(&args, &BTreeMap::new(), b"rustc fixture identity"),
            Err(WrapperError::UnsupportedDepInfoPathRemapping(option))
                if option == expected
        ));
    }
}

#[test]
fn exact_dep_info_pipeline_rejects_binary_dependencies_and_logical_env_injection() {
    let (_temp, source, dependency) = fixture_inputs();
    for suffix in [
        vec![OsString::from("-Zbinary-dep-depinfo")],
        vec![OsString::from("-Z"), OsString::from("binary-dep-depinfo")],
    ] {
        let mut arguments = rustc_args(&source, &dependency);
        arguments.extend(suffix);
        assert!(matches!(
            normalize_rustc_invocation(&arguments, &BTreeMap::new(), b"rustc fixture identity"),
            Err(WrapperError::UnsupportedBinaryDepDepInfo)
        ));
    }
    for suffix in [
        vec![OsString::from("--env-set=INJECTED=value")],
        vec![
            OsString::from("--env-set"),
            OsString::from("INJECTED=value"),
        ],
    ] {
        let mut arguments = rustc_args(&source, &dependency);
        arguments.extend(suffix);
        assert!(matches!(
            normalize_rustc_invocation(&arguments, &BTreeMap::new(), b"rustc fixture identity"),
            Err(WrapperError::UnsupportedLogicalEnvironmentInjection)
        ));
    }
}

#[test]
fn dep_info_output_must_remain_beneath_the_exact_target() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let outside = temp.path().join("outside.d");

    assert!(matches!(
        validate_dep_info_output_path(&target, &outside),
        Err(WrapperError::DepInfoOutsideTarget { .. })
    ));
}

#[cfg(unix)]
#[test]
fn dep_info_output_rejects_a_symlinked_parent() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, target.join("linked")).unwrap();

    assert!(matches!(
        validate_dep_info_output_path(&target, &target.join("linked/unit.d")),
        Err(WrapperError::UnsafeDepInfoOutput(path)) if path == target.join("linked")
    ));
}

#[cfg(unix)]
#[test]
fn descriptor_and_final_out_dir_roots_cannot_be_symlinks() {
    let temp = TempDir::new().unwrap();
    let real_target = temp.path().join("real-target");
    let linked_target = temp.path().join("linked-target");
    let out_dir = real_target.join("debug/build/fixture/out");
    fs::create_dir_all(&out_dir).unwrap();
    std::os::unix::fs::symlink(&real_target, &linked_target).unwrap();
    let linked_out_dir = linked_target.join("debug/build/fixture/out");

    assert!(matches!(
        descriptor_out_dir_coordinate(
            &linked_target,
            Some(linked_out_dir.to_str().unwrap()),
        ),
        Err(WrapperError::UnsafeOutDirDependency(path)) if path == linked_target
    ));
    assert!(matches!(
        validate_final_out_dir(&linked_target, Some(&linked_out_dir)),
        Err(WrapperError::UnsafeFinalTarget(path)) if path == linked_target
    ));
}

#[test]
fn explicit_package_source_closure_failures_do_not_fall_back() {
    let (temp, source, dependency) = fixture_inputs();
    let missing_package_root = temp.path().join("missing-package");
    let error = normalize_rustc_invocation_with_package_root(
        &rustc_args(&source, &dependency),
        &BTreeMap::new(),
        b"same toolchain",
        Some(&missing_package_root),
        None,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        WrapperError::HashPackageSourceClosure { path, .. }
            if path == missing_package_root
    ));
}

#[test]
fn descriptor_codegen_replaces_incremental_lto_and_object_outputs() {
    let args = vec![
        "--crate-name=fixture".into(),
        "--crate-type=lib".into(),
        "--emit=metadata,obj=/old/object.o,link".into(),
        "-Cincremental=/host/incremental".into(),
        "-C".into(),
        "lto=fat".into(),
        "-Clinker-plugin-lto".into(),
        "-Ccodegen-units=16".into(),
        "/host/source.rs".into(),
    ];
    let rewritten = descriptor_rustc_args(&args, Path::new("/staging/objects/unit.o")).unwrap();
    let rewritten = rewritten
        .iter()
        .map(|argument| argument.to_str().unwrap())
        .collect::<Vec<_>>();

    assert!(rewritten.contains(&"--emit=metadata,link"));
    assert!(rewritten.contains(&"--emit=obj=/staging/objects/unit.o"));
    assert!(rewritten.contains(&"-Ccodegen-units=1"));
    assert!(rewritten.contains(&"-Clto=off"));
    assert!(rewritten.contains(&"-Clink-dead-code=yes"));
    assert!(rewritten.contains(&"-Cstrip=none"));
    assert!(!rewritten
        .iter()
        .any(|argument| argument.contains("incremental")));
    assert!(!rewritten
        .iter()
        .any(|argument| argument.contains("linker-plugin-lto")));
    assert_eq!(
        rewritten
            .iter()
            .filter(|argument| argument.starts_with("--emit=obj="))
            .count(),
        1
    );
}

fn cargo_artifact(path: &str, features: &[&str]) -> CargoCompilerArtifact {
    CargoCompilerArtifact {
        package_id: "path+file:///workspace#app@0.1.0".to_string(),
        manifest_path: "/workspace/Cargo.toml".into(),
        target: CargoArtifactTarget {
            name: "app".to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: "/workspace/src/lib.rs".into(),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: features
            .iter()
            .map(|feature| (*feature).to_string())
            .collect(),
        filenames: BTreeSet::from([PathBuf::from(path)]),
        executable: None,
        fresh: false,
    }
}

fn expectation(unit_byte: u8, path: &str, features: &[&str]) -> CargoArtifactExpectation {
    CargoArtifactExpectation {
        unit: DescriptorCargoUnitId([unit_byte; 32]),
        package_id: "path+file:///workspace#app@0.1.0".to_string(),
        target_name: "app".to_string(),
        target_kinds: BTreeSet::from(["lib".to_string()]),
        crate_types: BTreeSet::from(["lib".to_string()]),
        features: features
            .iter()
            .map(|feature| (*feature).to_string())
            .collect(),
        profile: serde_json::json!({"opt_level":"0","test":false}),
        required_filenames: BTreeSet::from([PathBuf::from(path)]),
    }
}

#[test]
fn exact_artifact_match_rejects_ambiguity_and_duplicate_claims() {
    let artifacts = vec![
        cargo_artifact("/target/libapp.rlib", &[]),
        CargoCompilerArtifact {
            filenames: BTreeSet::from([
                PathBuf::from("/target/libapp.rlib"),
                PathBuf::from("/target/libapp.rmeta"),
            ]),
            ..cargo_artifact("/target/libapp.rmeta", &[])
        },
    ];
    let error =
        match_exact_cargo_artifacts(&artifacts, &[expectation(1, "/target/libapp.rlib", &[])])
            .unwrap_err();
    assert_eq!(
        error,
        ArtifactMatchError::AmbiguousArtifact {
            unit: DescriptorCargoUnitId([1; 32]),
            matches: 2,
        }
    );

    let artifacts = vec![cargo_artifact("/target/libapp.rlib", &[])];
    let error = match_exact_cargo_artifacts(
        &artifacts,
        &[
            expectation(1, "/target/libapp.rlib", &[]),
            expectation(2, "/target/libapp.rlib", &[]),
        ],
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ArtifactMatchError::ArtifactClaimedByMultipleUnits { .. }
    ));
}

fn manifest_unit_fixture(
    temp: &TempDir,
    unit_byte: u8,
    crate_name: &str,
    dependency_artifacts: BTreeMap<String, DescriptorDependencyArtifact>,
) -> (ValidatedDescriptorUnit, CargoCompilerArtifact, PathBuf) {
    let unit = DescriptorCargoUnitId([unit_byte; 32]);
    let package = temp.path().join(crate_name);
    let source = package.join("src/lib.rs");
    let manifest = package.join("Cargo.toml");
    let artifact = temp
        .path()
        .join("target/debug")
        .join(format!("lib{crate_name}.rlib"));
    let descriptor_object = temp.path().join(format!("objects/{unit}.o"));
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::create_dir_all(descriptor_object.parent().unwrap()).unwrap();
    fs::write(&source, format!("pub fn {crate_name}() {{}}\n")).unwrap();
    fs::write(&manifest, "[package]\nname='fixture'\nversion='0.0.0'\n").unwrap();
    fs::write(&artifact, format!("{crate_name} artifact bytes")).unwrap();
    fs::write(&descriptor_object, format!("{crate_name} descriptor bytes")).unwrap();

    let source_hash = hex_digest(&hash_file(&source, FILE_HASH_DOMAIN).unwrap());
    let validated = ValidatedDescriptorUnit {
        unit,
        sidecar: DescriptorUnitSidecar {
            schema_version: DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
            cargo_unit_id: unit.to_string(),
            cargo_manifest_path: Some(manifest.clone()),
            cargo_package_name: Some(crate_name.to_string()),
            cargo_package_version: Some("0.0.0".to_string()),
            cargo_profile: Some("debug".to_string()),
            cargo_profile_fingerprint: "11".repeat(32),
            cargo_primary_package: true,
            cargo_extra_filename: None,
            crate_name: crate_name.to_string(),
            crate_types: vec!["lib".to_string()],
            target: None,
            cfgs: vec![],
            features: vec![],
            source_hash,
            package_source_closure_hash: "33".repeat(32),
            dependency_artifacts,
            normalized_rustc_inputs_hash: unit.to_string(),
            logical_environment_hash: "66".repeat(32),
            dep_info_path: "debug/deps/fixture.d".to_string(),
            dep_info_working_directory: temp.path().canonicalize().unwrap(),
            out_dir: None,
            out_dir_dependencies: vec![],
            source_dependencies: vec![],
            environment_dependencies: vec![],
            realized_environment_hash: "55".repeat(32),
            descriptor_codegen: DescriptorCodegenSidecar {
                codegen_units: 1,
                incremental: false,
                link_dead_code: true,
                lto: "off".to_string(),
                strip: "none".to_string(),
            },
            descriptor_object: format!("objects/{unit}.o"),
            descriptor_object_hash: hex_digest(
                &hash_file(&descriptor_object, DESCRIPTOR_OBJECT_HASH_DOMAIN).unwrap(),
            ),
        },
        sidecar_path: temp.path().join(format!("units/{unit}.json")),
        descriptor_object,
    };
    let cargo = CargoCompilerArtifact {
        package_id: format!("path+file:///fixture#{crate_name}@0.0.0"),
        manifest_path: manifest,
        target: CargoArtifactTarget {
            name: crate_name.to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: source,
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([artifact.clone()]),
        executable: None,
        fresh: false,
    };
    (validated, cargo, artifact)
}

#[derive(Default)]
struct TrackingManifestHasher {
    single_paths: Vec<PathBuf>,
    dependency_paths: Vec<PathBuf>,
}

impl LinkManifestHasher for TrackingManifestHasher {
    fn hash_single(&mut self, path: &Path, domain: &[u8]) -> io::Result<[u8; 32]> {
        self.single_paths.push(path.to_path_buf());
        hash_file(path, domain)
    }

    fn hash_dependency(&mut self, path: &Path) -> io::Result<([u8; 32], [u8; 32])> {
        self.dependency_paths.push(path.to_path_buf());
        hash_file_two_domains(path, FILE_HASH_DOMAIN, LINK_ARTIFACT_HASH_DOMAIN)
    }
}

#[test]
fn link_manifest_hashes_only_exact_reachable_extern_paths() {
    let temp = TempDir::new().unwrap();
    let (dependency, mut dependency_cargo, dependency_path) =
        manifest_unit_fixture(&temp, 1, "dependency", BTreeMap::new());
    let same_bytes_but_unreferenced = temp.path().join("target/debug/libunreferenced.a");
    fs::write(
        &same_bytes_but_unreferenced,
        fs::read(&dependency_path).unwrap(),
    )
    .unwrap();
    dependency_cargo
        .filenames
        .insert(same_bytes_but_unreferenced.clone());
    let bulk_unreferenced = temp.path().join("target/debug/libbulk_unreferenced.a");
    File::create(&bulk_unreferenced)
        .unwrap()
        .set_len(16 * 1024 * 1024)
        .unwrap();
    dependency_cargo.filenames.insert(bulk_unreferenced.clone());
    let enumerated_dependency_bytes = dependency_cargo
        .filenames
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum::<u64>();

    let expected_hash = descriptor_input_content_hash(&dependency_path).unwrap();
    let (root, root_cargo, root_path) = manifest_unit_fixture(
        &temp,
        2,
        "root",
        BTreeMap::from([(
            "dependency".to_string(),
            DescriptorDependencyArtifact {
                path: dependency_path.clone(),
                content_hash: expected_hash,
            },
        )]),
    );
    let root_unit = root.unit;
    let output = DescriptorCargoOutput {
        cargo_artifacts: vec![dependency_cargo, root_cargo],
        units: vec![dependency, root],
    };
    let mut hasher = TrackingManifestHasher::default();

    let manifest = build_link_unit_manifest_with_hasher(
        InvocationId::derive("reachable-extern-hashing"),
        root_unit,
        &root_path,
        ArtifactRole::RootLibrary,
        &output,
        &mut hasher,
    )
    .unwrap();

    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(hasher.dependency_paths, vec![dependency_path]);
    assert!(!hasher.single_paths.contains(&same_bytes_but_unreferenced));
    assert!(!hasher.single_paths.contains(&bulk_unreferenced));
    let hashed_dependency_bytes = hasher
        .dependency_paths
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum::<u64>();
    assert!(enumerated_dependency_bytes >= 16 * 1024 * 1024);
    assert!(hashed_dependency_bytes < 1024);
}

#[test]
fn production_link_manifest_skips_unreachable_descriptor_objects() {
    let temp = TempDir::new().unwrap();
    let (dependency, dependency_cargo, dependency_path) =
        manifest_unit_fixture(&temp, 11, "dependency", BTreeMap::new());
    let reachable_object = dependency.descriptor_object.clone();
    let expected_hash = descriptor_input_content_hash(&dependency_path).unwrap();
    let (root, root_cargo, root_path) = manifest_unit_fixture(
        &temp,
        12,
        "root",
        BTreeMap::from([(
            "dependency".to_string(),
            DescriptorDependencyArtifact {
                path: dependency_path,
                content_hash: expected_hash,
            },
        )]),
    );
    let (unreachable, unreachable_cargo, _) =
        manifest_unit_fixture(&temp, 13, "unreachable", BTreeMap::new());
    let unreachable_object = unreachable.descriptor_object.clone();
    fs::remove_file(&unreachable_object).unwrap();

    let root_unit = root.unit;
    let output = DescriptorCargoOutput {
        cargo_artifacts: vec![dependency_cargo, root_cargo, unreachable_cargo],
        units: vec![dependency, root, unreachable],
    };

    let manifest = build_link_unit_manifest(
        InvocationId::derive("production-reachable-only-hashing"),
        root_unit,
        &root_path,
        ArtifactRole::RootLibrary,
        &output,
    )
    .unwrap();

    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(
        manifest
            .artifacts
            .iter()
            .filter(|artifact| artifact.role == ArtifactRole::DescriptorObject)
            .count(),
        2
    );
    assert!(manifest
        .artifacts
        .iter()
        .all(|artifact| artifact.path != unreachable_object.to_string_lossy()));

    fs::remove_file(&reachable_object).unwrap();
    let error = build_link_unit_manifest(
        InvocationId::derive("production-reachable-hashing-fails-closed"),
        root_unit,
        &root_path,
        ArtifactRole::RootLibrary,
        &output,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        LinkManifestAssemblyError::HashArtifact { path, .. }
            if path == reachable_object
    ));
}

#[test]
fn link_manifest_rechecks_reachable_extern_content_after_path_join() {
    let temp = TempDir::new().unwrap();
    let (dependency, dependency_cargo, dependency_path) =
        manifest_unit_fixture(&temp, 3, "dependency", BTreeMap::new());
    let (root, root_cargo, root_path) = manifest_unit_fixture(
        &temp,
        4,
        "root",
        BTreeMap::from([(
            "dependency".to_string(),
            DescriptorDependencyArtifact {
                path: dependency_path.clone(),
                content_hash: "aa".repeat(32),
            },
        )]),
    );
    let root_unit = root.unit;
    let output = DescriptorCargoOutput {
        cargo_artifacts: vec![dependency_cargo, root_cargo],
        units: vec![dependency, root],
    };

    let error = build_link_unit_manifest(
        InvocationId::derive("extern-content-recheck"),
        root_unit,
        &root_path,
        ArtifactRole::RootLibrary,
        &output,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        LinkManifestAssemblyError::DependencyArtifactHashMismatch { path, .. }
            if path == dependency_path
    ));
}

fn suffix_join_unit(
    temp: &TempDir,
    cargo_extra_filename: Option<&str>,
    cargo_primary_package: bool,
) -> ValidatedDescriptorUnit {
    let unit = DescriptorCargoUnitId([7; 32]);
    let source = temp.path().join("src/lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "pub fn suffix_fixture() {}\n").unwrap();
    let source_hash = hex_digest(&hash_file(&source, FILE_HASH_DOMAIN).unwrap());
    ValidatedDescriptorUnit {
        unit,
        sidecar: DescriptorUnitSidecar {
            schema_version: DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION,
            cargo_unit_id: unit.to_string(),
            cargo_manifest_path: Some(temp.path().join("Cargo.toml")),
            cargo_package_name: Some("suffix_fixture".to_string()),
            cargo_package_version: Some("0.0.0".to_string()),
            cargo_profile: Some("debug".to_string()),
            cargo_profile_fingerprint: "11".repeat(32),
            cargo_primary_package,
            cargo_extra_filename: cargo_extra_filename.map(str::to_string),
            crate_name: "suffix_fixture".to_string(),
            crate_types: vec!["lib".to_string()],
            target: None,
            cfgs: vec![],
            features: vec![],
            source_hash,
            package_source_closure_hash: "33".repeat(32),
            dependency_artifacts: BTreeMap::new(),
            normalized_rustc_inputs_hash: unit.to_string(),
            logical_environment_hash: "66".repeat(32),
            dep_info_path: "debug/deps/suffix_fixture.d".to_string(),
            dep_info_working_directory: temp.path().canonicalize().unwrap(),
            out_dir: None,
            out_dir_dependencies: vec![],
            source_dependencies: vec![],
            environment_dependencies: vec![],
            realized_environment_hash: "55".repeat(32),
            descriptor_codegen: DescriptorCodegenSidecar {
                codegen_units: 1,
                incremental: false,
                link_dead_code: true,
                lto: "off".to_string(),
                strip: "none".to_string(),
            },
            descriptor_object: format!("objects/{unit}.o"),
            descriptor_object_hash: "22".repeat(32),
        },
        sidecar_path: temp.path().join(format!("units/{unit}.json")),
        descriptor_object: temp.path().join(format!("objects/{unit}.o")),
    }
}

fn suffix_join_artifact(unit: &ValidatedDescriptorUnit, filename: &str) -> CargoCompilerArtifact {
    CargoCompilerArtifact {
        package_id: "path+file:///fixture#suffix_fixture@0.0.0".to_string(),
        manifest_path: unit
            .sidecar
            .cargo_manifest_path
            .clone()
            .expect("fixture manifest"),
        target: CargoArtifactTarget {
            name: "suffix_fixture".to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: unit
                .sidecar
                .cargo_manifest_path
                .as_ref()
                .unwrap()
                .parent()
                .unwrap()
                .join("src/lib.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([unit
            .sidecar
            .cargo_manifest_path
            .as_ref()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join(filename)]),
        executable: None,
        fresh: false,
    }
}

#[test]
fn final_unit_map_validates_exact_cargo_coordinates() {
    let temp = TempDir::new().unwrap();
    let unit = suffix_join_unit(&temp, None, true);
    let plan_id = PlanId::derive("unhashed-primary-plan");
    let plan_file_hash = format!("sha256:{}", "44".repeat(32));
    let written = write_content_addressed_final_unit_map(
        &temp.path().join("unit-maps"),
        plan_id,
        &plan_file_hash,
        std::slice::from_ref(&unit),
    )
    .unwrap();
    let map: FinalUnitMap = serde_json::from_slice(&fs::read(&written.path).unwrap()).unwrap();
    assert_eq!(map.entries.len(), 1);
    assert!(map.entries[0].coordinate.cargo_primary_package);
    assert_eq!(map.entries[0].coordinate.cargo_extra_filename, None);
    let serialized_map: serde_json::Value =
        serde_json::from_slice(&fs::read(&written.path).unwrap()).unwrap();
    assert!(serialized_map["entries"][0]["coordinate"]
        .get("package_source_closure_hash")
        .is_none());

    let mut collision = unit.clone();
    collision.unit = DescriptorCargoUnitId([8; 32]);
    collision.sidecar.cargo_unit_id = collision.unit.to_string();
    // The conservative package closure includes unobserved assets. A
    // change there must not select a different final Cargo unit after the
    // descriptor cache has already validated its exact observed inputs.
    collision.sidecar.package_source_closure_hash = "77".repeat(32);
    assert!(matches!(
        write_content_addressed_final_unit_map(
            &temp.path().join("colliding-unit-maps"),
            plan_id,
            &plan_file_hash,
            &[unit.clone(), collision.clone()],
        ),
        Err(FinalUnitMapError::AmbiguousCoordinate { .. })
    ));

    collision.sidecar.cargo_extra_filename = Some("-distinct-final-unit".to_string());
    let distinct = write_content_addressed_final_unit_map(
        &temp.path().join("distinct-unit-maps"),
        plan_id,
        &plan_file_hash,
        &[unit, collision.clone()],
    )
    .unwrap();
    let distinct_map: FinalUnitMap =
        serde_json::from_slice(&fs::read(distinct.path).unwrap()).unwrap();
    assert_eq!(distinct_map.entries.len(), 2);
    assert_eq!(
        distinct_map
            .entries
            .iter()
            .map(|entry| entry.coordinate.cargo_extra_filename.as_deref())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([None, Some("-distinct-final-unit")])
    );

    collision.sidecar.cargo_extra_filename = Some("../forged".to_string());
    assert!(matches!(
        write_content_addressed_final_unit_map(
            &temp.path().join("invalid-suffix-unit-maps"),
            plan_id,
            &plan_file_hash,
            &[collision],
        ),
        Err(FinalUnitMapError::InvalidCargoExtraFilename { .. })
    ));
}

#[test]
fn cargo_extra_filename_join_is_exact_and_fails_closed() {
    let temp = TempDir::new().unwrap();
    let unit = suffix_join_unit(&temp, Some("-aaa111"), false);
    let exact = suffix_join_artifact(&unit, "libsuffix_fixture-aaa111.rmeta");
    let other = suffix_join_artifact(&unit, "libsuffix_fixture-bbb222.rmeta");

    let joined = join_descriptor_units_to_cargo_artifacts(
        std::slice::from_ref(&unit),
        &[other.clone(), exact.clone()],
    )
    .unwrap();
    assert_eq!(joined[&unit.unit], exact);

    assert!(matches!(
        join_descriptor_units_to_cargo_artifacts(std::slice::from_ref(&unit), &[other]),
        Err(UnitArtifactJoinError::MissingArtifact(found)) if found == unit.unit
    ));

    let same_suffix_other_extension = suffix_join_artifact(&unit, "libsuffix_fixture-aaa111.rlib");
    assert!(matches!(
        join_descriptor_units_to_cargo_artifacts(
            std::slice::from_ref(&unit),
            &[exact, same_suffix_other_extension],
        ),
        Err(UnitArtifactJoinError::AmbiguousArtifact { unit: found, matches: 2 })
            if found == unit.unit
    ));

    let mut unsafe_unit = unit.clone();
    unsafe_unit.sidecar.cargo_extra_filename = Some("../forged".to_string());
    assert!(matches!(
        join_descriptor_units_to_cargo_artifacts(&[unsafe_unit], &[]),
        Err(UnitArtifactJoinError::InvalidCargoExtraFilename { .. })
    ));

    let mut missing_unit = unit;
    missing_unit.sidecar.cargo_extra_filename = None;
    assert!(matches!(
        join_descriptor_units_to_cargo_artifacts(&[missing_unit], &[]),
        Err(UnitArtifactJoinError::MissingArtifact(_))
    ));
}

#[test]
fn primary_unit_with_suffix_never_falls_back_to_unhashed_output() {
    let temp = TempDir::new().unwrap();
    let unit = suffix_join_unit(&temp, Some("-aaa111"), true);
    let exact = suffix_join_artifact(&unit, "libsuffix_fixture-aaa111.rmeta");
    let unhashed = suffix_join_artifact(&unit, "libsuffix_fixture.rmeta");

    let joined = join_descriptor_units_to_cargo_artifacts(
        std::slice::from_ref(&unit),
        &[unhashed.clone(), exact.clone()],
    )
    .unwrap();
    assert_eq!(joined[&unit.unit], exact);

    assert!(matches!(
        join_descriptor_units_to_cargo_artifacts(std::slice::from_ref(&unit), &[unhashed]),
        Err(UnitArtifactJoinError::MissingArtifact(found)) if found == unit.unit
    ));
}

#[test]
fn cargo_extra_filename_matcher_requires_the_exact_basename_segment() {
    let suffix = "-abc123";
    assert!(artifact_filename_has_cargo_extra_filename(
        Path::new("/target/libwidget-abc123.rlib"),
        "widget",
        suffix,
    ));
    assert!(artifact_filename_has_cargo_extra_filename(
        Path::new("/target/widget-abc123"),
        "widget",
        suffix,
    ));
    assert!(!artifact_filename_has_cargo_extra_filename(
        Path::new("/target/libwidget-abc123-forged.rlib"),
        "widget",
        suffix,
    ));
    assert!(!artifact_filename_has_cargo_extra_filename(
        Path::new("/target/prefix-libwidget-abc123.rlib"),
        "widget",
        suffix,
    ));
    assert!(validate_cargo_extra_filename("-abc123").is_ok());
    assert!(validate_cargo_extra_filename("").is_err());
    assert!(validate_cargo_extra_filename("../abc123").is_err());
    assert!(validate_cargo_extra_filename("-abc\n123").is_err());
}

#[test]
fn unhashed_primary_outputs_cover_hyphenated_bins_and_primary_libraries() {
    let executable = PathBuf::from("/target/proa-playground");
    let artifact = CargoCompilerArtifact {
        executable: Some(executable.clone()),
        filenames: BTreeSet::from([executable]),
        ..cargo_artifact("/target/unused", &[])
    };

    assert!(artifact_has_unhashed_primary_output(
        &artifact,
        "proa_playground",
    ));

    let temp = TempDir::new().unwrap();
    let mut binary_unit = suffix_join_unit(&temp, Some("-abc123"), true);
    binary_unit.sidecar.crate_name = "proa_playground".to_string();
    binary_unit.sidecar.crate_types = vec!["bin".to_string()];
    let mut binary_artifact = suffix_join_artifact(&binary_unit, "proa-playground");
    binary_artifact.target.name = "proa-playground".to_string();
    binary_artifact.target.kind = BTreeSet::from(["bin".to_string()]);
    binary_artifact.target.crate_types = BTreeSet::from(["bin".to_string()]);
    binary_artifact.filenames = artifact.filenames.clone();
    binary_artifact.executable = artifact.executable.clone();
    let joined = join_descriptor_units_to_cargo_artifacts(
        &[binary_unit.clone()],
        &[binary_artifact.clone()],
    )
    .unwrap();
    assert_eq!(joined[&binary_unit.unit], binary_artifact);

    let unit = suffix_join_unit(&temp, None, true);
    let library = suffix_join_artifact(&unit, "libsuffix_fixture.rlib");
    let joined = join_descriptor_units_to_cargo_artifacts(
        std::slice::from_ref(&unit),
        std::slice::from_ref(&library),
    )
    .unwrap();
    assert_eq!(joined[&unit.unit], library);
}

#[test]
fn unhashed_dependency_cdylib_output_joins_its_cargo_artifact() {
    let temp = TempDir::new().unwrap();
    let mut unit = suffix_join_unit(&temp, None, false);
    unit.sidecar.crate_types = vec!["cdylib".to_string(), "rlib".to_string()];
    let mut artifact = suffix_join_artifact(&unit, "libsuffix_fixture.rlib");
    artifact.target.kind = BTreeSet::from(["cdylib".to_string(), "rlib".to_string()]);
    artifact.target.crate_types = BTreeSet::from(["cdylib".to_string(), "rlib".to_string()]);

    let joined = join_descriptor_units_to_cargo_artifacts(
        std::slice::from_ref(&unit),
        std::slice::from_ref(&artifact),
    )
    .unwrap();

    assert_eq!(joined[&unit.unit], artifact);
}

#[test]
fn malformed_and_duplicate_cargo_messages_fail_closed() {
    let mut collector = CargoArtifactCollector::default();
    assert!(matches!(
        collector.ingest_line("not json"),
        Err(CargoMessageError::MalformedJson { .. })
    ));

    let artifact = serde_json::json!({
        "reason": "compiler-artifact",
        "package_id": "path+file:///workspace#app@0.1.0",
        "manifest_path": "/workspace/Cargo.toml",
        "target": {
            "name": "app",
            "kind": ["lib"],
            "crate_types": ["lib"],
            "src_path": "/workspace/src/lib.rs",
            "edition": "2021"
        },
        "profile": {"opt_level":"0","test":false},
        "features": [],
        "filenames": ["/target/libapp.rlib"],
        "executable": null,
        "fresh": false
    })
    .to_string();
    let mut collector = CargoArtifactCollector::default();
    collector.ingest_line(&artifact).unwrap();
    assert!(matches!(
        collector.ingest_line(&artifact),
        Err(CargoMessageError::DuplicateCompilerArtifact { .. })
    ));

    let mut collector = CargoArtifactCollector::default();
    assert!(matches!(
        collector.ingest_line(r#"{"reason":"compiler-artifact","package_id":"x"}"#),
        Err(CargoMessageError::MalformedCompilerArtifact { .. })
    ));
    assert!(matches!(
        CargoArtifactCollector::default().finish(),
        Err(CargoMessageError::MissingBuildFinished)
    ));
    let mut collector = CargoArtifactCollector::default();
    assert!(matches!(
        collector.ingest_line(r#"{"reason":"future-cargo-message"}"#),
        Err(CargoMessageError::UnknownReason { .. })
    ));
    let mut collector = CargoArtifactCollector::default();
    assert!(matches!(
        collector.ingest_line(r#"{"reason":"compiler-message","package_id":"x"}"#),
        Err(CargoMessageError::MalformedAuxiliaryMessage { .. })
    ));
}

#[test]
fn collector_returns_only_cargo_enumerated_artifact_paths() {
    let artifact = serde_json::json!({
        "reason": "compiler-artifact",
        "package_id": "path+file:///workspace#app@0.1.0",
        "manifest_path": "/workspace/Cargo.toml",
        "target": {
            "name": "app",
            "kind": ["lib"],
            "crate_types": ["lib"],
            "src_path": "/workspace/src/lib.rs",
            "edition": "2021"
        },
        "profile": {"opt_level":"0","test":false},
        "features": ["feature-a"],
        "filenames": ["/exact/cargo/path/libapp.rlib"],
        "executable": null,
        "fresh": true
    })
    .to_string();
    let mut collector = CargoArtifactCollector::default();
    collector.ingest_line(&artifact).unwrap();
    collector
        .ingest_line(r#"{"reason":"build-finished","success":true}"#)
        .unwrap();
    let artifacts = collector.finish().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(
        artifacts[0].filenames,
        BTreeSet::from([PathBuf::from("/exact/cargo/path/libapp.rlib")])
    );

    let matched = match_exact_cargo_artifacts(
        &artifacts,
        &[expectation(
            9,
            "/exact/cargo/path/libapp.rlib",
            &["feature-a"],
        )],
    )
    .unwrap();
    assert_eq!(
        matched[&DescriptorCargoUnitId([9; 32])].filenames,
        artifacts[0].filenames
    );
}

#[test]
fn source_dependency_equivalence_accepts_renamed_tool_tree_outputs() {
    let dep = |path: &str, hash: &str| DescriptorSourceDependency {
        absolute_path: PathBuf::from(path),
        content_hash: hash.to_owned(),
    };
    let expected = [
        dep("/repo/proa_bench/src/lib.rs", "hash-lib"),
        dep(
            "/repo/target/proa-rsjs/descriptor/out/templates/aa-11",
            "hash-t1",
        ),
        dep(
            "/repo/target/proa-rsjs/descriptor/out/templates/aa-22",
            "hash-t2",
        ),
    ];
    let renamed = [
        dep("/repo/proa_bench/src/lib.rs", "hash-lib"),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-33",
            "hash-t2",
        ),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-44",
            "hash-t1",
        ),
    ];
    assert!(source_dependency_sets_equivalent(&expected, &renamed));

    // A changed hash under the tool tree is a real divergence.
    let tampered = [
        dep("/repo/proa_bench/src/lib.rs", "hash-lib"),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-33",
            "hash-t1",
        ),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-44",
            "hash-OTHER",
        ),
    ];
    assert!(!source_dependency_sets_equivalent(&expected, &tampered));

    // Paths outside the tool tree never get the relaxation, even with
    // identical content hashes.
    let moved_source = [
        dep("/repo/proa_bench/src/other.rs", "hash-lib"),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-33",
            "hash-t1",
        ),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-44",
            "hash-t2",
        ),
    ];
    assert!(!source_dependency_sets_equivalent(&expected, &moved_source));

    // A generated file leaving the tool tree must not silently pass.
    let escaped = [
        dep("/repo/proa_bench/src/lib.rs", "hash-lib"),
        dep("/repo/target/plain-cargo/out/templates/bb-33", "hash-t1"),
        dep(
            "/repo/target/proa-rsjs/final-target/out/templates/bb-44",
            "hash-t2",
        ),
    ];
    assert!(!source_dependency_sets_equivalent(&expected, &escaped));
}

#[test]
fn uplifted_root_lib_aliases_its_hashed_deps_rlib() {
    let temp = TempDir::new().unwrap();
    let deps = temp.path().join("debug/deps");
    fs::create_dir_all(&deps).unwrap();
    let uplifted = temp.path().join("debug/libatlas_fixture.rlib");
    let rmeta = deps.join("libatlas_fixture-aaa111.rmeta");
    let hashed_rlib = deps.join("libatlas_fixture-aaa111.rlib");
    fs::write(&uplifted, b"rlib").unwrap();
    fs::write(&rmeta, b"rmeta").unwrap();

    let artifact = CargoCompilerArtifact {
        package_id: "path+file:///fixture#atlas-fixture@0.0.0".to_string(),
        manifest_path: temp.path().join("Cargo.toml"),
        target: CargoArtifactTarget {
            name: "atlas-fixture".to_string(),
            kind: BTreeSet::from(["lib".to_string()]),
            crate_types: BTreeSet::from(["lib".to_string()]),
            src_path: temp.path().join("src/lib.rs"),
            edition: "2021".to_string(),
        },
        profile: serde_json::json!({"opt_level":"0","test":false}),
        features: BTreeSet::new(),
        filenames: BTreeSet::from([uplifted.clone(), rmeta.clone()]),
        executable: None,
        fresh: false,
    };

    // The hashed rlib is not on disk yet: no alias may be invented.
    assert_eq!(uplifted_root_lib_hashed_rlib(&artifact), None);

    fs::write(&hashed_rlib, b"rlib").unwrap();
    assert_eq!(
        uplifted_root_lib_hashed_rlib(&artifact),
        Some(hashed_rlib.clone())
    );

    // A dependency-shaped artifact (hashed rlib enumerated directly, no
    // uplifted filename) must not produce an alias.
    let mut dependency = artifact.clone();
    dependency.filenames = BTreeSet::from([hashed_rlib, rmeta]);
    assert_eq!(uplifted_root_lib_hashed_rlib(&dependency), None);

    // A bin-only artifact never aliases.
    let mut bin = artifact;
    bin.target.crate_types = BTreeSet::from(["bin".to_string()]);
    assert_eq!(uplifted_root_lib_hashed_rlib(&bin), None);
}
