// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn artifact_export_replaces_the_destination_and_preserves_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("linked-server");
    let destination = temp.path().join("dist/server");
    fs::write(&source, b"new linked executable").unwrap();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, b"stale executable").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source, fs::Permissions::from_mode(0o751)).unwrap();
    }

    copy_executable_atomically(&source, &destination).unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"new linked executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o751
        );
    }
    assert_eq!(
        fs::read_dir(destination.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn relative_artifact_output_is_resolved_from_the_project_root() {
    assert_eq!(
        resolve_artifact_output(Path::new("/workspace/site"), Path::new("dist/server")),
        PathBuf::from("/workspace/site/dist/server")
    );
}

#[test]
fn artifact_output_warning_matches_the_conservative_source_closure() {
    let package = Path::new("/workspace/site/app");
    let cargo_target = Path::new("/workspace/site/build-target");

    assert!(artifact_output_enters_source_closure(
        Path::new("/workspace/site/app/dist/server"),
        package,
        cargo_target,
    ));
    assert!(artifact_output_enters_source_closure(
        Path::new("/workspace/site/app/target/../dist/server"),
        package,
        cargo_target,
    ));
    assert!(!artifact_output_enters_source_closure(
        Path::new("/workspace/site/dist/server"),
        package,
        cargo_target,
    ));
    assert!(!artifact_output_enters_source_closure(
        Path::new("/workspace/site/build-target/export/server"),
        package,
        cargo_target,
    ));
    assert!(!artifact_output_enters_source_closure(
        Path::new("/workspace/site/app/target/export/server"),
        package,
        cargo_target,
    ));
    assert!(!artifact_output_enters_source_closure(
        Path::new("/workspace/site/app/vendor/node_modules/tool/server"),
        package,
        cargo_target,
    ));
}

#[test]
fn linked_build_parser_preserves_supported_cargo_selection() {
    let command = [
        "cargo",
        "build",
        "--profile",
        "release-fast",
        "-p",
        "demo",
        "--bin",
        "server",
        "--features",
        "hydration,metrics",
        "-Fdebug-panel",
        "--target",
        "aarch64-apple-darwin",
        "--offline",
        "--locked",
        "--jobs",
        "4",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_build_command(&command).unwrap();

    assert_eq!(parsed.package.as_deref(), Some("demo"));
    assert_eq!(parsed.bin.as_deref(), Some("server"));
    assert_eq!(parsed.profile, "release-fast");
    assert_eq!(parsed.target.as_deref(), Some("aarch64-apple-darwin"));
    assert_eq!(
        parsed.features,
        BTreeSet::from([
            "debug-panel".to_string(),
            "hydration".to_string(),
            "metrics".to_string(),
        ])
    );
    assert!(parsed.offline);
    assert!(parsed.locked);
    assert_eq!(
        parsed.cargo_args,
        command[1..].iter().map(OsString::from).collect::<Vec<_>>()
    );
    assert_eq!(
        parsed.metadata_flags,
        [OsString::from("--offline"), OsString::from("--locked")]
    );
    assert_eq!(parsed.selection_args, ["--jobs", "4"]);
}

#[test]
fn linked_build_parser_preserves_one_exact_example_root() {
    let command = [
        "cargo",
        "build",
        "--locked",
        "-p",
        "fixture_app",
        "--example",
        "example_server",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_build_command(&command).unwrap();

    assert_eq!(parsed.package.as_deref(), Some("fixture_app"));
    assert_eq!(parsed.bin, None);
    assert_eq!(parsed.example.as_deref(), Some("example_server"));
    assert_eq!(
        parsed.cargo_args,
        command[1..].iter().map(OsString::from).collect::<Vec<_>>()
    );

    let conflicting = parse_cargo_build_command(
        &[
            "cargo",
            "build",
            "-p",
            "fixture_app",
            "--bin",
            "server",
            "--example",
            "example_server",
        ]
        .map(str::to_string),
    )
    .unwrap_err()
    .to_string();
    assert!(conflicting.contains("either `--bin"), "{conflicting}");

    let plural = parse_cargo_build_command(&["cargo", "build", "--examples"].map(str::to_string))
        .unwrap_err()
        .to_string();
    assert!(plural.contains("multiple"), "{plural}");
}

#[test]
fn linked_check_parser_preserves_final_check_and_uses_the_same_selection_model() {
    let command = [
        "cargo",
        "check",
        "--profile",
        "release-fast",
        "-p",
        "demo",
        "--bin",
        "server",
        "--features",
        "hydration,metrics",
        "--target",
        "aarch64-apple-darwin",
        "--offline",
        "--locked",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_command(&command, LinkedCargoAction::Check).unwrap();

    assert_eq!(parsed.cargo_args[0], OsString::from("check"));
    assert_eq!(parsed.package.as_deref(), Some("demo"));
    assert_eq!(parsed.bin.as_deref(), Some("server"));
    assert_eq!(parsed.profile, "release-fast");
    assert_eq!(parsed.target.as_deref(), Some("aarch64-apple-darwin"));
    assert_eq!(
        parsed.features,
        BTreeSet::from(["hydration".to_string(), "metrics".to_string()])
    );
    assert!(parsed.offline);
    assert!(parsed.locked);
}

#[test]
fn linked_dev_parser_separates_cargo_selection_from_server_arguments() {
    let command = [
        "cargo",
        "run",
        "-p",
        "demo",
        "--bin",
        "server",
        "--features",
        "hydration",
        "--",
        "--seed",
        "42",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_command(&command, LinkedCargoAction::Dev).unwrap();

    assert_eq!(
        parsed.cargo_args,
        command[1..8].iter().map(OsString::from).collect::<Vec<_>>()
    );
    assert_eq!(
        parsed.runtime_args,
        [OsString::from("--seed"), OsString::from("42")]
    );
    assert_eq!(parsed.package.as_deref(), Some("demo"));
    assert_eq!(parsed.bin.as_deref(), Some("server"));
    assert_eq!(parsed.features, BTreeSet::from(["hydration".to_string()]));
}

#[test]
fn linked_run_parser_preserves_selection_and_runtime_arguments() {
    let command = [
        "cargo",
        "run",
        "--profile",
        "release-fast",
        "-p",
        "demo",
        "--bin",
        "server",
        "--features",
        "hydration,metrics",
        "--target",
        "aarch64-apple-darwin",
        "--locked",
        "--offline",
        "--",
        "--seed",
        "42",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_command(&command, LinkedCargoAction::Run).unwrap();

    assert_eq!(parsed.cargo_args[0], OsString::from("run"));
    assert_eq!(parsed.package.as_deref(), Some("demo"));
    assert_eq!(parsed.bin.as_deref(), Some("server"));
    assert_eq!(parsed.profile, "release-fast");
    assert_eq!(parsed.target.as_deref(), Some("aarch64-apple-darwin"));
    assert_eq!(
        parsed.features,
        BTreeSet::from(["hydration".to_string(), "metrics".to_string()])
    );
    assert!(parsed.locked);
    assert!(parsed.offline);
    assert_eq!(
        parsed.runtime_args,
        [OsString::from("--seed"), OsString::from("42")]
    );
    assert!(!parsed
        .cargo_args
        .iter()
        .any(|argument| argument == OsStr::new("--")));
}

#[test]
fn linked_test_parser_requires_one_exact_target_and_preserves_harness_arguments() {
    let command = [
        "cargo",
        "test",
        "-p",
        "demo",
        "--test",
        "http",
        "--features",
        "integration",
        "--",
        "--nocapture",
        "linked_plan",
    ]
    .map(str::to_string);
    let parsed = parse_cargo_command(&command, LinkedCargoAction::Test).unwrap();

    assert_eq!(parsed.cargo_args[0], OsString::from("test"));
    assert_eq!(parsed.package.as_deref(), Some("demo"));
    assert_eq!(parsed.bin, None);
    assert_eq!(parsed.test.as_deref(), Some("http"));
    assert_eq!(parsed.profile, "test");
    assert_eq!(
        parsed.runtime_args,
        [OsString::from("--nocapture"), OsString::from("linked_plan")]
    );

    let missing = parse_cargo_command(
        &["cargo", "test", "-p", "demo"].map(str::to_string),
        LinkedCargoAction::Test,
    )
    .unwrap_err()
    .to_string();
    assert!(missing.contains("exactly one"), "{missing}");

    let conflicting = parse_cargo_command(
        &[
            "cargo", "test", "-p", "demo", "--bin", "server", "--test", "http",
        ]
        .map(str::to_string),
        LinkedCargoAction::Test,
    )
    .unwrap_err()
    .to_string();
    assert!(conflicting.contains("exactly one"), "{conflicting}");
}

#[test]
fn linked_build_parser_rejects_reserved_and_multi_root_arguments() {
    let target_dir = parse_cargo_build_command(
        &["cargo", "build", "--target-dir", "custom"].map(str::to_string),
    )
    .unwrap_err()
    .to_string();
    assert!(target_dir.contains("reserved"), "{target_dir}");

    let workspace =
        parse_cargo_build_command(&["cargo", "build", "--workspace"].map(str::to_string))
            .unwrap_err()
            .to_string();
    assert!(workspace.contains("multiple"), "{workspace}");

    let custom = parse_cargo_build_command(&["just", "deploy"].map(str::to_string))
        .unwrap_err()
        .to_string();
    assert!(custom.contains("Custom non-Cargo"), "{custom}");
}

#[test]
fn metadata_root_selection_never_guesses_between_binaries() {
    let metadata = CargoMetadata {
        packages: vec![CargoMetadataPackage {
            id: "path+file:///fixture#app@0.1.0".to_string(),
            name: "app".to_string(),
            version: "0.1.0".to_string(),
            manifest_path: PathBuf::from("/fixture/Cargo.toml"),
            targets: vec![
                CargoMetadataTarget {
                    name: "admin".to_string(),
                    kind: vec!["bin".to_string()],
                },
                CargoMetadataTarget {
                    name: "public".to_string(),
                    kind: vec!["bin".to_string()],
                },
            ],
        }],
        workspace_members: vec!["path+file:///fixture#app@0.1.0".to_string()],
        workspace_default_members: vec!["path+file:///fixture#app@0.1.0".to_string()],
        target_directory: PathBuf::from("/fixture/target"),
    };
    let ambiguous =
        parse_cargo_build_command(&["cargo", "build", "-p", "app"].map(str::to_string)).unwrap();
    let error = select_exact_cargo_root(&metadata, &ambiguous, LinkedCargoAction::Build)
        .unwrap_err()
        .to_string();
    assert!(error.contains("will not guess"), "{error}");

    let selected = parse_cargo_build_command(
        &["cargo", "build", "-p", "app@0.1.0", "--bin", "public"].map(str::to_string),
    )
    .unwrap();
    let root = select_exact_cargo_root(&metadata, &selected, LinkedCargoAction::Build).unwrap();
    assert_eq!(root.package_id, "path+file:///fixture#app@0.1.0");
    assert_eq!(root.target_name, "public");
    assert_eq!(root.target_kinds, BTreeSet::from(["bin".to_string()]));
}

#[test]
fn metadata_test_root_selection_uses_the_explicit_target_kind_and_name() {
    let metadata = CargoMetadata {
        packages: vec![CargoMetadataPackage {
            id: "path+file:///fixture#app@0.1.0".to_string(),
            name: "app".to_string(),
            version: "0.1.0".to_string(),
            manifest_path: PathBuf::from("/fixture/Cargo.toml"),
            targets: vec![
                CargoMetadataTarget {
                    name: "app".to_string(),
                    kind: vec!["bin".to_string()],
                },
                CargoMetadataTarget {
                    name: "http".to_string(),
                    kind: vec!["test".to_string()],
                },
                CargoMetadataTarget {
                    name: "database".to_string(),
                    kind: vec!["test".to_string()],
                },
            ],
        }],
        workspace_members: vec!["path+file:///fixture#app@0.1.0".to_string()],
        workspace_default_members: vec!["path+file:///fixture#app@0.1.0".to_string()],
        target_directory: PathBuf::from("/fixture/target"),
    };
    let parsed = parse_cargo_command(
        &[
            "cargo",
            "test",
            "-p",
            "app",
            "--test",
            "database",
            "--offline",
        ]
        .map(str::to_string),
        LinkedCargoAction::Test,
    )
    .unwrap();
    let root = select_exact_cargo_root(&metadata, &parsed, LinkedCargoAction::Test).unwrap();

    assert_eq!(root.target_name, "database");
    assert_eq!(root.requested_target_kind, "test");
    assert_eq!(root.target_kinds, BTreeSet::from(["test".to_string()]));
}

#[test]
fn metadata_build_root_selection_uses_the_exact_example_kind_and_name() {
    let metadata = CargoMetadata {
        packages: vec![CargoMetadataPackage {
            id: "path+file:///fixture#fixture_app@0.1.0".to_string(),
            name: "fixture_app".to_string(),
            version: "0.1.0".to_string(),
            manifest_path: PathBuf::from("/fixture/Cargo.toml"),
            targets: vec![
                CargoMetadataTarget {
                    name: "fixture_app".to_string(),
                    kind: vec!["lib".to_string()],
                },
                CargoMetadataTarget {
                    name: "example_server".to_string(),
                    kind: vec!["example".to_string()],
                },
            ],
        }],
        workspace_members: vec!["path+file:///fixture#fixture_app@0.1.0".to_string()],
        workspace_default_members: vec!["path+file:///fixture#fixture_app@0.1.0".to_string()],
        target_directory: PathBuf::from("/fixture/target"),
    };
    let parsed = parse_cargo_build_command(
        &[
            "cargo",
            "build",
            "-p",
            "fixture_app",
            "--example",
            "example_server",
            "--locked",
        ]
        .map(str::to_string),
    )
    .unwrap();
    let root = select_exact_cargo_root(&metadata, &parsed, LinkedCargoAction::Build).unwrap();

    assert_eq!(root.target_name, "example_server");
    assert_eq!(root.requested_target_kind, "example");
    assert_eq!(root.target_kinds, BTreeSet::from(["example".to_string()]));
}

#[test]
fn metadata_root_selection_rejects_multiple_default_packages() {
    let package = |name: &str| CargoMetadataPackage {
        id: format!("path+file:///fixture#{name}@0.1.0"),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        manifest_path: PathBuf::from(format!("/fixture/{name}/Cargo.toml")),
        targets: vec![CargoMetadataTarget {
            name: name.to_string(),
            kind: vec!["bin".to_string()],
        }],
    };
    let metadata = CargoMetadata {
        packages: vec![package("admin"), package("public")],
        workspace_members: vec![
            "path+file:///fixture#admin@0.1.0".to_string(),
            "path+file:///fixture#public@0.1.0".to_string(),
        ],
        workspace_default_members: vec![
            "path+file:///fixture#admin@0.1.0".to_string(),
            "path+file:///fixture#public@0.1.0".to_string(),
        ],
        target_directory: PathBuf::from("/fixture/target"),
    };
    let parsed = parse_cargo_build_command(&["cargo", "build"].map(str::to_string)).unwrap();
    let error = select_exact_cargo_root(&metadata, &parsed, LinkedCargoAction::Build)
        .unwrap_err()
        .to_string();
    assert!(error.contains("2 workspace packages"), "{error}");
}

#[test]
fn descriptor_producer_version_is_explicitly_versioned() {
    assert_eq!(RSJS_COMPILER_PRODUCER, ProducerVersion::new(0, 2, 0));
}

#[test]
fn build_command_uses_profile_override() {
    let command = build_command(&BuildOptions {
        root: None,
        profile: Some("release-fast".to_string()),
        package: Some("demo".to_string()),
        bin: Some("server".to_string()),
        example: None,
        target: Some("aarch64-apple-darwin".to_string()),
        features: vec!["hydration".to_string(), "metrics".to_string()],
        all_features: false,
        no_default_features: true,
        locked: true,
        offline: true,
        no_check: false,
        no_assets: false,
        artifact_out: None,
        command: Vec::new(),
    });

    assert_eq!(
        command,
        [
            "cargo",
            "build",
            "--profile",
            "release-fast",
            "-p",
            "demo",
            "--bin",
            "server",
            "--target",
            "aarch64-apple-darwin",
            "--features",
            "hydration,metrics",
            "--no-default-features",
            "--locked",
            "--offline"
        ]
    );
}

#[test]
fn dev_command_honors_configured_application_binary() {
    let options = DevOptions {
        root: None,
        host: None,
        port: None,
        watch: Vec::new(),
        open: false,
        offline: true,
        no_assets: false,
        hot_static: false,
        no_restart: false,
        no_reload: false,
        no_css_hot_swap: false,
        reload_port: None,
        debounce_ms: None,
        command: Vec::new(),
    };
    let config = ProjectConfig {
        application: Some(crate::config::ApplicationConfig {
            binary: "incident-desk".into(),
        }),
        ..Default::default()
    };
    assert_eq!(
        dev_command(&options, &config),
        ["cargo", "run", "--bin", "incident-desk", "--offline"]
    );
}

#[test]
fn generated_public_output_check_never_swallows_post_snapshot_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let public = Path::new("public");
    fs::create_dir_all(root.join(public)).unwrap();
    let css = PathBuf::from("public/styles.css");
    let logo = PathBuf::from("public/logo.svg");
    fs::write(root.join(&css), "body{color:black}").unwrap();
    fs::write(root.join(&logo), "<svg/>").unwrap();

    let config = ProjectConfig {
        build: crate::config::BuildConfig {
            assets: Some(vec![AssetConfig {
                kind: "tailwind".into(),
                input: PathBuf::from("styles/input.css"),
                output: css.clone(),
                ..Default::default()
            }]),
            ..Default::default()
        },
        ..Default::default()
    };

    let watch_paths = vec![public.to_path_buf()];
    let snapshot = watch_snapshot(root, &watch_paths).unwrap();

    // Case 1: Declared generated output rewritten with identical bytes (newer stamp).
    let before = snapshot_generated_public_outputs(root, &config, public, &snapshot).unwrap();
    assert!(before.contains_key(&css));
    assert!(!before.contains_key(&logo));
    std::thread::sleep(Duration::from_millis(20));
    fs::write(root.join(&css), "body{color:black}").unwrap();
    let post = watch_snapshot(root, &watch_paths).unwrap();
    assert!(all_unchanged_generated_outputs(root, &[&css], &before, &post).unwrap());

    // Case 2: Authored file saved after `snapshot` is never considered unchanged.
    fs::write(root.join(&logo), "<svg><circle/></svg>").unwrap();
    let post_with_logo = watch_snapshot(root, &watch_paths).unwrap();
    assert!(
        !all_unchanged_generated_outputs(root, &[&css, &logo], &before, &post_with_logo).unwrap()
    );

    // Case 3: Declared output edited between `snapshot` and `before` is skipped.
    fs::write(root.join(&css), "body{color:white}").unwrap();
    let before_after_edit =
        snapshot_generated_public_outputs(root, &config, public, &snapshot).unwrap();
    assert!(!before_after_edit.contains_key(&css));
}

#[cfg(unix)]
#[test]
fn watch_snapshot_records_dangling_symlinks_without_aborting() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let public = root.join("public");
    fs::create_dir_all(&public).unwrap();
    let broken = public.join("missing.svg");
    symlink(public.join("does-not-exist.svg"), &broken).unwrap();

    let snapshot = watch_snapshot(root, &[PathBuf::from("public")]).unwrap();
    assert!(snapshot.contains_key(Path::new("public/missing.svg")));
}

#[test]
fn check_command_uses_check_with_exact_cargo_selection() {
    let command = check_command(&CheckOptions {
        root: None,
        profile: Some("release-fast".to_string()),
        package: Some("demo".to_string()),
        bin: Some("server".to_string()),
        target: Some("aarch64-apple-darwin".to_string()),
        features: vec!["hydration".to_string(), "metrics".to_string()],
        all_features: false,
        no_default_features: true,
        locked: true,
        offline: true,
    });

    assert_eq!(
        command,
        [
            "cargo",
            "check",
            "--profile",
            "release-fast",
            "-p",
            "demo",
            "--bin",
            "server",
            "--target",
            "aarch64-apple-darwin",
            "--features",
            "hydration,metrics",
            "--no-default-features",
            "--locked",
            "--offline"
        ]
    );
}

#[test]
fn run_and_test_commands_place_execution_arguments_after_cargo_selection() {
    let run = run_command(&RunOptions {
        root: None,
        profile: Some("release-fast".to_string()),
        package: Some("demo".to_string()),
        bin: Some("server".to_string()),
        target: None,
        features: vec!["hydration".to_string()],
        all_features: false,
        no_default_features: false,
        locked: true,
        offline: true,
        arguments: vec!["--seed".to_string(), "42".to_string()],
    });
    assert_eq!(
        run,
        [
            "cargo",
            "run",
            "--profile",
            "release-fast",
            "-p",
            "demo",
            "--bin",
            "server",
            "--features",
            "hydration",
            "--locked",
            "--offline",
            "--",
            "--seed",
            "42",
        ]
    );

    let test = test_command(&TestOptions {
        root: None,
        profile: None,
        package: Some("demo".to_string()),
        bin: None,
        test: Some("http".to_string()),
        target: None,
        features: Vec::new(),
        all_features: false,
        no_default_features: true,
        locked: false,
        offline: true,
        watch: false,
        watch_paths: Vec::new(),
        keep_database: false,
        arguments: vec!["--nocapture".to_string(), "linked_plan".to_string()],
    });
    assert_eq!(
        test,
        [
            "cargo",
            "test",
            "-p",
            "demo",
            "--no-default-features",
            "--offline",
            "--test",
            "http",
            "--",
            "--nocapture",
            "linked_plan",
        ]
    );
}

#[test]
fn dev_watch_debounce_requires_one_stable_window_and_resets_on_new_bytes() {
    let snapshot = |len, modified_nanos| {
        BTreeMap::from([(
            PathBuf::from("src/main.rs"),
            FileStamp {
                len,
                modified_nanos,
            },
        )])
    };
    let baseline = snapshot(10, 100);
    let first_change = snapshot(11, 200);
    let second_change = snapshot(12, 300);
    let second_seen = Duration::from_millis(10) + DEV_WATCH_DEBOUNCE - Duration::from_millis(50);
    let mut debounce = DevWatchDebouncer::new(baseline.clone(), DEV_WATCH_DEBOUNCE);

    assert_eq!(
        debounce.observe(Duration::from_millis(10), first_change.clone()),
        DevWatchDecision::Debouncing
    );
    assert_eq!(
        debounce.observe(second_seen - Duration::from_millis(10), first_change),
        DevWatchDecision::Debouncing
    );
    assert_eq!(
        debounce.observe(second_seen, second_change.clone()),
        DevWatchDecision::Debouncing
    );
    assert_eq!(
        debounce.observe(
            second_seen + DEV_WATCH_DEBOUNCE - Duration::from_millis(1),
            second_change.clone()
        ),
        DevWatchDecision::Debouncing
    );
    assert_eq!(
        debounce.observe(second_seen + DEV_WATCH_DEBOUNCE, second_change.clone()),
        DevWatchDecision::Ready(second_change.clone())
    );

    debounce.commit(second_change.clone());
    assert_eq!(
        debounce.observe(Duration::from_millis(900), second_change),
        DevWatchDecision::Idle
    );
    assert_eq!(
        debounce.observe(Duration::from_millis(910), baseline),
        DevWatchDecision::Debouncing
    );
}

fn dev_options(debounce_ms: Option<u64>) -> DevOptions {
    DevOptions {
        root: None,
        host: None,
        port: None,
        watch: vec![],
        open: false,
        offline: false,
        no_assets: false,
        hot_static: false,
        no_restart: false,
        no_reload: false,
        no_css_hot_swap: false,
        reload_port: None,
        debounce_ms,
        command: Vec::new(),
    }
}

#[test]
fn browser_urls_use_connectable_hosts_and_bracket_ipv6() {
    for (host, expected) in [
        ("127.0.0.1", "http://127.0.0.1:4321"),
        ("localhost", "http://localhost:4321"),
        ("0.0.0.0", "http://127.0.0.1:4321"),
        ("::", "http://[::1]:4321"),
        ("::1", "http://[::1]:4321"),
        ("[::1]", "http://[::1]:4321"),
    ] {
        assert_eq!(dev_url(host, 4321), expected);
    }
}

#[test]
fn the_debounce_window_is_tunable_but_cannot_undercut_the_poll_interval() {
    assert_eq!(
        dev_watch_debounce(&dev_options(None)).unwrap(),
        DEV_WATCH_DEBOUNCE
    );
    assert_eq!(
        dev_watch_debounce(&dev_options(Some(1500))).unwrap(),
        Duration::from_millis(1500)
    );
    assert_eq!(
        dev_watch_debounce(&dev_options(Some(100))).unwrap(),
        DEV_WATCH_POLL
    );

    // Anything below one poll cannot be honoured, and silently rounding it up
    // would misreport what the loop is doing.
    let error = dev_watch_debounce(&dev_options(Some(20)))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("below the 100ms file-watch poll interval"),
        "{error}"
    );
    assert!(dev_watch_debounce(&dev_options(Some(0))).is_err());
}

#[test]
fn changed_paths_reports_edits_additions_and_deletions() {
    let stamp = |len| FileStamp {
        len,
        modified_nanos: len as u128,
    };
    let before = BTreeMap::from([
        (PathBuf::from("src/main.rs"), stamp(1)),
        (PathBuf::from("styles/input.css"), stamp(2)),
        (PathBuf::from("src/gone.rs"), stamp(3)),
    ]);
    let after = BTreeMap::from([
        (PathBuf::from("src/main.rs"), stamp(1)),
        (PathBuf::from("styles/input.css"), stamp(9)),
        (PathBuf::from("src/new.rs"), stamp(4)),
    ]);

    let mut changed = changed_paths(&before, &after);
    changed.sort();
    assert_eq!(
        changed,
        vec![
            PathBuf::from("src/gone.rs"),
            PathBuf::from("src/new.rs"),
            PathBuf::from("styles/input.css"),
        ]
    );
}

#[test]
fn only_stylesheet_edits_skip_the_rebuild() {
    let paths = |names: &[&str]| names.iter().map(PathBuf::from).collect::<Vec<_>>();

    assert!(is_stylesheet_only_change(&paths(&[
        "styles/input.css",
        "styles/theme.CSS"
    ])));
    // A Tailwind class edited inside a template still changes a compiled
    // input, so the server has to be rebuilt.
    assert!(!is_stylesheet_only_change(&paths(&[
        "styles/input.css",
        "src/pages/home.rs"
    ])));
    assert!(!is_stylesheet_only_change(&paths(&["Cargo.toml"])));
    assert!(!is_stylesheet_only_change(&paths(&["src/docs/index.mdx"])));
    // An empty change set means the poller saw nothing; it must not be
    // mistaken for a stylesheet-only edit.
    assert!(!is_stylesheet_only_change(&[]));
}

#[test]
fn persistent_namespace_reuses_source_cycles_and_splits_features_and_test_mode() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
             name = \"linked_dev_namespace\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n\n\
             [features]\n\
             alternate = []\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let command = ["cargo", "run", "--offline"].map(str::to_string);
    let wrapper = compile_test_wrapper(&root);

    let first =
        prepare_linked_cargo_with_wrapper(&root, &command, LinkedCargoAction::Dev, wrapper.clone())
            .unwrap();
    crate::rsjs::pipeline::validate_linked_cargo_request(&first.request).unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let source_changed =
        prepare_linked_cargo_with_wrapper(&root, &command, LinkedCargoAction::Dev, wrapper.clone())
            .unwrap();

    assert_eq!(first.request.invocation, source_changed.request.invocation);
    assert_eq!(
        first.public_output_directory,
        source_changed.public_output_directory
    );
    assert!(first.public_output_directory.starts_with(
        first
            .request
            .descriptor
            .source_exclusion_directory
            .join("proa-output")
    ));
    assert_eq!(
        first.descriptor_cache_directory,
        source_changed.descriptor_cache_directory
    );
    assert_eq!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&source_changed.request)
    );
    let (cache_name, target_name) = if cfg!(windows) {
        (
            format!("c{DESCRIPTOR_CACHE_SCHEMA_VERSION}"),
            format!("d{DESCRIPTOR_TARGET_POLICY_VERSION}-{DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION}-{PERSISTENT_DESCRIPTOR_TARGET_LAYOUT_VERSION}"),
        )
    } else {
        (
            format!("descriptor-cache-v{DESCRIPTOR_CACHE_SCHEMA_VERSION}"),
            format!("descriptor-target-policy-v{DESCRIPTOR_TARGET_POLICY_VERSION}-sidecar-v{DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION}-persistent-v{PERSISTENT_DESCRIPTOR_TARGET_LAYOUT_VERSION}"),
        )
    };
    assert!(first.descriptor_cache_directory.ends_with(cache_name));
    assert!(descriptor_target_dir(&first.request).ends_with(target_name));
    let retired_one_shot_target = descriptor_target_dir(&first.request)
            .parent()
            .unwrap()
            .join(format!(
                "descriptor-target-policy-v{DESCRIPTOR_TARGET_POLICY_VERSION}-sidecar-v{DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION}"
            ));
    let retired_nonce_target = retired_one_shot_target.join("retired-nonce");
    fs::create_dir_all(&retired_nonce_target).unwrap();
    fs::write(retired_nonce_target.join("artifact"), b"retired").unwrap();
    assert!(!descriptor_target_dir(&first.request).starts_with(&retired_one_shot_target));
    crate::rsjs::build::prepare_descriptor_target_directory(descriptor_target_dir(&first.request))
        .unwrap();
    assert_ne!(
        first.request.descriptor.staging_directory,
        source_changed.request.descriptor.staging_directory
    );
    assert_eq!(first.request.final_cargo_args[0], OsString::from("build"));

    let featured = prepare_linked_cargo_with_wrapper(
        &root,
        &[
            "cargo".to_string(),
            "run".to_string(),
            "--offline".to_string(),
            "--features".to_string(),
            "alternate".to_string(),
        ],
        LinkedCargoAction::Dev,
        wrapper.clone(),
    )
    .unwrap();
    assert_ne!(first.request.invocation, featured.request.invocation);
    assert_ne!(
        first.public_output_directory,
        featured.public_output_directory
    );
    assert_eq!(
        first.descriptor_cache_directory,
        featured.descriptor_cache_directory
    );
    assert_eq!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&featured.request)
    );
    assert_eq!(
        first.request.final_target_root,
        featured.request.final_target_root
    );

    let linked_run = prepare_linked_cargo_with_wrapper(
        &root,
        &[
            "cargo".to_string(),
            "run".to_string(),
            "--offline".to_string(),
            "--".to_string(),
            "--probe".to_string(),
        ],
        LinkedCargoAction::Run,
        wrapper.clone(),
    )
    .unwrap();
    crate::rsjs::pipeline::validate_linked_cargo_request(&linked_run.request).unwrap();
    assert_eq!(first.request.invocation, linked_run.request.invocation);
    assert_eq!(
        first.public_output_directory,
        linked_run.public_output_directory
    );
    assert_eq!(
        first.descriptor_cache_directory,
        linked_run.descriptor_cache_directory
    );
    assert_eq!(
        linked_run.request.descriptor.cargo_args[0],
        OsString::from("build")
    );
    assert_eq!(
        linked_run.request.final_cargo_args,
        [
            OsString::from("run"),
            OsString::from("--offline"),
            OsString::from("--"),
            OsString::from("--probe"),
        ]
    );

    let linked_test = prepare_linked_cargo_with_wrapper(
        &root,
        &[
            "cargo".to_string(),
            "test".to_string(),
            "--bin".to_string(),
            "linked_dev_namespace".to_string(),
            "--offline".to_string(),
            "--".to_string(),
            "--nocapture".to_string(),
        ],
        LinkedCargoAction::Test,
        wrapper,
    )
    .unwrap();
    crate::rsjs::pipeline::validate_linked_cargo_request(&linked_test.request).unwrap();
    assert_ne!(first.request.invocation, linked_test.request.invocation);
    assert_eq!(
        first.descriptor_cache_directory,
        linked_test.descriptor_cache_directory
    );
    assert_eq!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&linked_test.request)
    );
    assert_eq!(
        first.request.final_target_root,
        linked_test.request.final_target_root
    );
    assert_eq!(linked_test.request.root.role, ArtifactRole::RootTest);
    assert_eq!(
        linked_test.request.descriptor.cargo_args[0],
        OsString::from("test")
    );
    assert!(linked_test
        .request
        .descriptor
        .cargo_args
        .iter()
        .any(|argument| argument == OsStr::new("--no-run")));
    assert_eq!(
        linked_test.request.final_cargo_args,
        [
            OsString::from("test"),
            OsString::from("--bin"),
            OsString::from("linked_dev_namespace"),
            OsString::from("--offline"),
            OsString::from("--"),
            OsString::from("--nocapture"),
        ]
    );
}

#[cfg(not(windows))]
#[test]
fn same_path_wrapper_replacement_splits_persistent_cache_namespaces() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
             name = \"linked_wrapper_fingerprint\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let wrapper = compile_test_wrapper(&root);
    let command = ["cargo", "build", "--offline"].map(str::to_string);

    let wrapper_v1 = fs::read(&wrapper).unwrap();
    assert!(!wrapper_v1.is_empty());
    let first = prepare_linked_cargo_with_wrapper(
        &root,
        &command,
        LinkedCargoAction::Build,
        wrapper.clone(),
    )
    .unwrap();
    let first_snapshot = first.request.descriptor.wrapper.clone();
    let first_fingerprint = wrapper_executable_fingerprint(&first_snapshot).unwrap();
    let repeated = prepare_linked_cargo_with_wrapper(
        &root,
        &command,
        LinkedCargoAction::Build,
        wrapper.clone(),
    )
    .unwrap();
    assert_eq!(repeated.request.descriptor.wrapper, first_snapshot);
    assert_eq!(repeated.request.invocation, first.request.invocation);

    // Preserve both the absolute path and byte length. Cache separation
    // must come from content, not a path or file-size heuristic. The first
    // request must also retain the attested bytes after this replacement.
    let mut wrapper_v2 = wrapper_v1.clone();
    let last = wrapper_v2.last_mut().unwrap();
    *last ^= 1;
    fs::write(&wrapper, &wrapper_v2).unwrap();
    assert_eq!(fs::read(&first_snapshot).unwrap(), wrapper_v1);
    let replaced = prepare_linked_cargo_with_wrapper(
        &root,
        &command,
        LinkedCargoAction::Build,
        wrapper.clone(),
    )
    .unwrap();
    let replaced_snapshot = replaced.request.descriptor.wrapper.clone();
    assert_eq!(
        first.public_output_directory,
        replaced.public_output_directory
    );
    let replaced_fingerprint = wrapper_executable_fingerprint(&replaced_snapshot).unwrap();

    // Reusing an existing snapshot still verifies its current bytes, even
    // when corruption preserves file size and the mutable source is valid.
    fs::write(&wrapper, &wrapper_v1).unwrap();
    let mut corrupted = wrapper_v1.clone();
    *corrupted.last_mut().unwrap() ^= 2;
    fs::write(&first_snapshot, &corrupted).unwrap();
    let error = prepare_linked_cargo_with_wrapper(
        &root,
        &command,
        LinkedCargoAction::Build,
        wrapper.clone(),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("content-addressed final Cargo artifact conflicts"));
    fs::write(&first_snapshot, &wrapper_v1).unwrap();

    assert_ne!(first_snapshot, wrapper);
    assert_ne!(replaced_snapshot, wrapper);
    assert_ne!(first_snapshot, replaced_snapshot);
    assert_eq!(fs::read(&first_snapshot).unwrap(), wrapper_v1);
    assert_eq!(fs::read(&replaced_snapshot).unwrap(), wrapper_v2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        assert_ne!(
            fs::metadata(&first_snapshot).unwrap().permissions().mode() & 0o111,
            0
        );
        assert_ne!(
            fs::metadata(&replaced_snapshot)
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
    assert_ne!(first_fingerprint, replaced_fingerprint);
    assert_ne!(first.request.invocation, replaced.request.invocation);
    assert_ne!(
        first.descriptor_cache_directory,
        replaced.descriptor_cache_directory
    );
    assert_ne!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&replaced.request)
    );
}

#[test]
fn linked_build_request_preserves_the_exact_example_root() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("examples")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
             name = \"linked_example_root\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn library() {}\n").unwrap();
    fs::write(
        temp.path().join("examples/example_server.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let command = [
        "cargo",
        "build",
        "--offline",
        "-p",
        "linked_example_root",
        "--example",
        "example_server",
    ]
    .map(str::to_string);

    let prepared = prepare_linked_cargo_with_wrapper(
        &root,
        &command,
        LinkedCargoAction::Build,
        compile_test_wrapper(&root),
    )
    .unwrap();

    assert_eq!(prepared.request.root.target_name, "example_server");
    assert_eq!(
        prepared.request.root.target_kinds,
        BTreeSet::from(["example".to_string()])
    );
    assert_eq!(prepared.request.root.role, ArtifactRole::RootExecutable);
    let expected_args = command[1..].iter().map(OsString::from).collect::<Vec<_>>();
    assert_eq!(
        &prepared.request.descriptor.cargo_args[..expected_args.len()],
        expected_args.as_slice()
    );
    assert_eq!(prepared.request.final_cargo_args, expected_args);
}

fn descriptor_target_dir(request: &LinkedCargoRequest) -> &Path {
    &request.descriptor.target_directory
}

fn compile_test_wrapper(directory: &Path) -> PathBuf {
    let source = directory.join("cargo-proa-test-wrapper.rs");
    let executable = directory.join(format!(
        "cargo-proa-test-wrapper{}",
        std::env::consts::EXE_SUFFIX
    ));
    // Distinct fixture bytes keep the corruption test's snapshot private
    // while other lifecycle tests run against the same Cargo target cache.
    let fixture = format!(
        "fn main() {{ std::hint::black_box({:?}); }}\n",
        directory.to_string_lossy()
    );
    fs::write(&source, fixture).unwrap();
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let output = Command::new(rustc)
        .args(["--edition=2021", "-C", "debuginfo=0", "-C", "opt-level=s"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile a small valid wrapper fixture");
    assert!(
        output.status.success(),
        "wrapper fixture compilation failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::canonicalize(executable).unwrap()
}

#[test]
fn asset_command_builds_tailwind_invocation() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("styles")).unwrap();
    fs::write(temp.path().join("styles/input.css"), "").unwrap();
    fs::write(temp.path().join("styles/tailwind.config.cjs"), "").unwrap();

    let command = asset_command(
        temp.path(),
        &AssetConfig {
            kind: "tailwind".to_string(),
            input: PathBuf::from("styles/input.css"),
            output: PathBuf::from("public/styles.css"),
            config: Some(PathBuf::from("styles/tailwind.config.cjs")),
            command: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(
        command,
        [
            "tailwindcss",
            "-i",
            "styles/input.css",
            "-o",
            "public/styles.css",
            "-c",
            "styles/tailwind.config.cjs"
        ]
    );
}

#[test]
fn missing_tailwind_cli_message_offers_platform_installation() {
    let message = missing_program_message("tailwindcss");

    if cfg!(windows) {
        assert!(message.contains("tailwindcss.exe"));
        assert!(!message.contains("curl"));
    } else {
        assert!(message.contains("--tailwind-only"));
        assert!(message.contains("brew install tailwindcss"));
    }
    assert!(!message.contains("npx"));
}

#[test]
fn test_environment_layers_env_test_and_forces_the_isolated_database() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join(".env"),
        "PROA_TEST_LAYER=base\nPROA_TEST_BASE_ONLY=present\nDATABASE_URL=sqlite://wrong.db\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".env.test"),
        "PROA_TEST_LAYER=test\nPROA_TEST_ONLY=yes\n",
    )
    .unwrap();
    let database = IsolatedTestDatabase {
        run_directory: temp.path().join("target/proa-tests/run"),
        path: temp.path().join("target/proa-tests/run/test.db"),
    };

    let environment = test_environment(temp.path(), Some(&database)).unwrap();

    assert_eq!(
        environment.get(OsStr::new("PROA_TEST_LAYER")),
        Some(&OsString::from("test"))
    );
    assert_eq!(
        environment.get(OsStr::new("PROA_TEST_BASE_ONLY")),
        Some(&OsString::from("present"))
    );
    assert_eq!(
        environment.get(OsStr::new("PROA_TEST_ONLY")),
        Some(&OsString::from("yes"))
    );
    assert_eq!(
        environment.get(OsStr::new("DATABASE_URL")),
        Some(&database.url())
    );
}

#[test]
fn invalid_test_environment_entry_is_rejected_with_its_line() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join(".env.test"), "NOT AN ENV LINE\n").unwrap();

    let error = test_environment(temp.path(), None).unwrap_err().to_string();

    assert!(error.contains(".env.test:1"), "{error}");
}

#[test]
fn integration_test_discovery_matches_cargo_top_level_targets() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("tests/nested")).unwrap();
    fs::create_dir_all(temp.path().join("tests/directory_target")).unwrap();
    fs::write(temp.path().join("tests/alpha.rs"), "").unwrap();
    fs::write(temp.path().join("tests/beta.rs"), "").unwrap();
    fs::write(temp.path().join("tests/nested/helper.rs"), "").unwrap();
    fs::write(temp.path().join("tests/directory_target/main.rs"), "").unwrap();

    assert_eq!(
        discover_integration_test_targets(temp.path()).unwrap(),
        ["alpha", "beta", "directory_target"]
    );
}

#[cfg(unix)]
#[test]
fn stop_child_preserves_graceful_exit_and_full_grace_before_forcing_exit() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    let temp = tempfile::tempdir().unwrap();
    for (name, trap) in [("graceful", "exit 0"), ("stubborn", "")] {
        let ready = temp.path().join(name);
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "trap \"$1\" TERM; : > \"$2\"; while :; do sleep 0.02; done",
                "stop-fixture",
                trap,
            ])
            .arg(&ready)
            .process_group(0)
            .spawn()
            .unwrap();
        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            if Instant::now() >= ready_deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("shutdown fixture did not become ready");
            }
            thread::sleep(Duration::from_millis(5));
        }
        let start = Instant::now();
        stop_child(&mut child).unwrap();
        let status = child.wait().unwrap();
        if name == "graceful" {
            assert!(
                status.success(),
                "TERM trap should finish normally: {status}"
            );
        } else {
            assert_eq!(status.signal(), Some(libc::SIGKILL));
            assert!(start.elapsed() >= Duration::from_secs(2));
        }
    }
}

#[cfg(unix)]
mod dev_preparation_tests {
    use super::*;

    const SOURCE: &str =
        r#"fn page() { html_hot_sync! { "page"; <p class="old">"hello"{self.value}</p> }; }"#;

    fn fixture(before: Option<&str>, asset: Option<&str>) -> (tempfile::TempDir, ProjectConfig) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("styles")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("src/main.rs"), SOURCE).unwrap();
        fs::write(root.join("src/generated.rs"), "const GENERATED: u8 = 1;").unwrap();
        // Same-length edits must change the stamp even on coarse-timestamp filesystems.
        for source in ["src/main.rs", "src/generated.rs"] {
            fs::File::open(root.join(source))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
                .unwrap();
        }
        fs::write(root.join("generator-input"), "const GENERATED: u8 = 1;").unwrap();
        fs::write(root.join("styles/input.css"), ".old { color: red; }").unwrap();
        let mut config = crate::config::ProjectConfig::default();
        config.paths.public = None;
        if let Some(command) = before {
            config.scripts.before.dev = vec!["generate".into()];
            config.scripts.named.insert(
                "generate".into(),
                crate::config::ScriptConfig {
                    command: vec!["sh".into(), "-c".into(), command.into()],
                    ..Default::default()
                },
            );
        }
        config.build.assets = Some(
            asset
                .into_iter()
                .map(|command| AssetConfig {
                    kind: "command".into(),
                    command: vec!["sh".into(), "-c".into(), command.into()],
                    input: "styles/input.css".into(),
                    output: "public/styles.css".into(),
                    config: None,
                })
                .collect(),
        );
        fs::write(
            root.join("proa.config.rs"),
            crate::config::generated_rust_config(&config).unwrap(),
        )
        .unwrap();
        (temp, config)
    }

    fn manifest(state: &crate::hot_static::HotState) -> PathBuf {
        PathBuf::from(state.environment().into_values().next().unwrap())
    }

    #[test]
    fn watcher_observes_links_without_following_public_directory_cycles() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("public")).unwrap();
        let watched = [PathBuf::from("public")];
        let before = watch_snapshot(temp.path(), &watched).unwrap();
        std::os::unix::fs::symlink(".", temp.path().join("public/loop")).unwrap();
        let after = watch_snapshot(temp.path(), &watched).unwrap();
        assert_eq!(
            changed_paths(&before, &after),
            [PathBuf::from("public/loop")]
        );
        assert_eq!(after.len(), 1);
        assert!(public_assets::publish(
            temp.path(),
            Some(Path::new("public")),
            &temp.path().join("output")
        )
        .is_err());
        fs::create_dir(temp.path().join("shared")).unwrap();
        fs::write(temp.path().join("shared/file.txt"), "one").unwrap();
        std::os::unix::fs::symlink("../shared", temp.path().join("public/shared")).unwrap();
        let before = watch_snapshot(temp.path(), &watched).unwrap();
        fs::write(temp.path().join("shared/file.txt"), "different").unwrap();
        let after = watch_snapshot(temp.path(), &watched).unwrap();
        assert_eq!(
            changed_paths(&before, &after),
            [PathBuf::from("public/shared/file.txt")]
        );
    }

    #[test]
    fn generated_identical_rust_is_consumed_without_another_reload() {
        let (temp, _config) = fixture(Some("cp generator-input src/generated.rs"), None);
        let root = temp.path();
        let active = crate::hot_static::HotState::prepare(root).unwrap();
        let committed = watch_snapshot(root, &default_watch_paths()).unwrap();
        fs::write(root.join("src/main.rs"), SOURCE.replace("old", "new")).unwrap();
        let trigger = watch_snapshot(root, &default_watch_paths()).unwrap();
        let mut failure = trigger.clone();
        let mut inputs = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            Some(&active),
            &mut failure,
            &mut default_watch_paths(),
        )
        .unwrap();
        assert_ne!(
            trigger[Path::new("src/generated.rs")],
            inputs.snapshot[Path::new("src/generated.rs")]
        );
        inputs.hot_update.as_mut().unwrap().activate().unwrap();
        let mut watch = DevWatchDebouncer::new(inputs.snapshot, DEV_WATCH_DEBOUNCE);
        assert_eq!(
            watch.observe(
                Duration::ZERO,
                watch_snapshot(root, &default_watch_paths()).unwrap()
            ),
            DevWatchDecision::Idle
        );
    }

    #[test]
    fn failed_tasks_or_assets_leave_the_active_template_unchanged() {
        for (before, asset) in [
            (Some("cp generator-input src/generated.rs; exit 1"), None),
            (Some("cp generator-input src/generated.rs"), Some("exit 1")),
        ] {
            let (temp, _config) = fixture(before, asset);
            let root = temp.path();
            let active = crate::hot_static::HotState::prepare(root).unwrap();
            let published = fs::read(manifest(&active)).unwrap();
            let committed = watch_snapshot(root, &default_watch_paths()).unwrap();
            fs::write(root.join("src/main.rs"), SOURCE.replace("old", "new")).unwrap();
            let mut failure = watch_snapshot(root, &default_watch_paths()).unwrap();
            assert!(prepare_dev_inputs(
                root,
                load_project_config(root).unwrap(),
                &dev_options(None),
                &committed,
                Some(&active),
                &mut failure,
                &mut default_watch_paths(),
            )
            .is_err());
            assert_eq!(fs::read(manifest(&active)).unwrap(), published);
            assert_eq!(
                fs::read_dir(manifest(&active).parent().unwrap())
                    .unwrap()
                    .count(),
                1
            );
            let mut watch = DevWatchDebouncer::new(failure, DEV_WATCH_DEBOUNCE);
            assert_eq!(
                watch.observe(
                    Duration::ZERO,
                    watch_snapshot(root, &default_watch_paths()).unwrap()
                ),
                DevWatchDecision::Idle
            );
        }
    }

    #[test]
    fn edits_during_publication_are_still_pending() {
        let (temp, _config) = fixture(None, None);
        let root = temp.path();
        let active = crate::hot_static::HotState::prepare(root).unwrap();
        let committed = watch_snapshot(root, &default_watch_paths()).unwrap();
        let published = fs::read(manifest(&active)).unwrap();
        fs::write(root.join("src/main.rs"), SOURCE.replace("old", "new")).unwrap();
        let mut failure = watch_snapshot(root, &default_watch_paths()).unwrap();
        let mut inputs = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            Some(&active),
            &mut failure,
            &mut default_watch_paths(),
        )
        .unwrap();
        assert_eq!(fs::read(manifest(&active)).unwrap(), published);
        fs::write(
            root.join("src/main.rs"),
            SOURCE.replace("old", "later-save"),
        )
        .unwrap();
        inputs.hot_update.as_mut().unwrap().activate().unwrap();
        assert_ne!(fs::read(manifest(&active)).unwrap(), published);
        let mut watch = DevWatchDebouncer::new(inputs.snapshot, DEV_WATCH_DEBOUNCE);
        assert_eq!(
            watch.observe(
                Duration::ZERO,
                watch_snapshot(root, &default_watch_paths()).unwrap()
            ),
            DevWatchDecision::Debouncing
        );
    }

    #[test]
    fn css_cycle_with_new_generated_rust_requires_a_linked_rebuild() {
        let (temp, _config) = fixture(Some("cp generator-input src/generated.rs"), None);
        let root = temp.path();
        let active = crate::hot_static::HotState::prepare(root).unwrap();
        let committed = watch_snapshot(root, &default_watch_paths()).unwrap();
        fs::write(root.join("generator-input"), "const GENERATED: u8 = 2;").unwrap();
        fs::write(root.join("styles/input.css"), ".new { color: blue; }").unwrap();
        let mut failure = watch_snapshot(root, &default_watch_paths()).unwrap();
        let inputs = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            Some(&active),
            &mut failure,
            &mut default_watch_paths(),
        )
        .unwrap();
        assert!(!inputs.css_only);
        assert!(inputs.hot_update.is_none());
    }

    #[test]
    fn failed_preparation_keeps_the_snapshot_and_watch_paths_in_agreement() {
        let (temp, mut config) = fixture(None, Some("exit 1"));
        let root = temp.path();
        let mut watch_paths = default_watch_paths();
        let committed = watch_snapshot(root, &watch_paths).unwrap();
        config.paths.public = Some("generated".into());
        fs::create_dir(root.join("generated")).unwrap();
        fs::write(root.join("generated/output.rs"), "const OUTPUT: u8 = 1;").unwrap();
        fs::write(
            root.join("proa.config.rs"),
            crate::config::generated_rust_config(&config).unwrap(),
        )
        .unwrap();
        let mut failure = watch_snapshot(root, &watch_paths).unwrap();
        assert!(prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths
        )
        .is_err());
        assert_eq!(
            watch_paths,
            dev_watch_paths(&config, &dev_options(None)).unwrap()
        );
        let mut watch = DevWatchDebouncer::new(failure, DEV_WATCH_DEBOUNCE);
        assert_eq!(
            watch.observe(Duration::ZERO, watch_snapshot(root, &watch_paths).unwrap()),
            DevWatchDecision::Idle
        );
    }

    #[test]
    fn stylesheet_edits_during_asset_build_are_not_swallowed() {
        let (temp, _config) = fixture(None, Some("cp late.css styles/input.css"));
        let root = temp.path();
        let committed = watch_snapshot(root, &default_watch_paths()).unwrap();
        fs::write(root.join("styles/input.css"), ".new { color: blue; }").unwrap();
        fs::write(root.join("late.css"), ".later { color: green; }").unwrap();
        let mut failure = watch_snapshot(root, &default_watch_paths()).unwrap();
        let inputs = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut default_watch_paths(),
        )
        .unwrap();
        assert!(inputs.css_only);
        let mut watch = DevWatchDebouncer::new(inputs.snapshot, DEV_WATCH_DEBOUNCE);
        assert_eq!(
            watch.observe(
                Duration::ZERO,
                watch_snapshot(root, &default_watch_paths()).unwrap()
            ),
            DevWatchDecision::Debouncing
        );
    }

    #[test]
    fn publishing_the_new_default_public_directory_requires_a_catalog_restart() {
        let (temp, mut config) = fixture(None, Some("cp styles/input.css public/styles.css"));
        let root = temp.path();
        config.paths.public = Some("public".into());
        fs::write(
            root.join("proa.config.rs"),
            crate::config::generated_rust_config(&config).unwrap(),
        )
        .unwrap();
        let loaded = load_project_config(root).unwrap();
        let mut watch_paths = dev_watch_paths(&loaded, &dev_options(None)).unwrap();
        assert!(watch_paths.contains(&PathBuf::from("public")));
        assert!(!root.join("public").exists());
        let committed = watch_snapshot(root, &watch_paths).unwrap();
        fs::write(root.join("styles/input.css"), ".new { color: blue; }").unwrap();
        let mut failure = committed.clone();
        let inputs = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(!inputs.css_only);
        assert!(inputs.hot_update.is_none());
        assert_eq!(
            fs::read(root.join("public/styles.css")).unwrap(),
            b".new { color: blue; }"
        );
    }

    #[test]
    fn start_manifest_preserves_multiple_binaries_and_separates_cross_targets() {
        let temp = tempfile::tempdir().unwrap();
        let target_dir = temp.path().join("target");
        let host_a = default_release_export_path(&target_dir, "web", None);
        let host_b = default_release_export_path(&target_dir, "admin", None);
        let cross_linux =
            default_release_export_path(&target_dir, "web", Some("x86_64-unknown-linux-gnu"));
        let cross_win =
            default_release_export_path(&target_dir, "web", Some("x86_64-pc-windows-msvc"));

        assert_ne!(host_a, cross_linux);
        assert_eq!(
            cross_linux,
            target_dir.join("x86_64-unknown-linux-gnu/release/web")
        );
        assert_eq!(
            cross_win,
            target_dir.join("x86_64-pc-windows-msvc/release/web.exe")
        );

        let assets_a = target_dir.join("proa-output/web/aaa");
        let assets_b = target_dir.join("proa-output/admin/bbb");
        let assets_cross = target_dir.join("proa-output/web/ccc");

        public_assets::record_start_manifest(
            &target_dir,
            "pkg-a",
            "web",
            "release",
            None,
            &host_a,
            Some(&assets_a),
        )
        .unwrap();
        public_assets::record_start_manifest(
            &target_dir,
            "pkg-b",
            "admin",
            "release",
            None,
            &host_b,
            Some(&assets_b),
        )
        .unwrap();
        public_assets::record_start_manifest(
            &target_dir,
            "pkg-a",
            "web",
            "release",
            Some("x86_64-unknown-linux-gnu"),
            &cross_linux,
            Some(&assets_cross),
        )
        .unwrap();

        let entries = public_assets::load_start_manifest(&target_dir);
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.binary == "web" && entry.target_triple.is_none())
                .and_then(|entry| entry.public_assets.as_deref()),
            Some(assets_a.as_path())
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.binary == "admin" && entry.target_triple.is_none())
                .and_then(|entry| entry.public_assets.as_deref()),
            Some(assets_b.as_path())
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.target_triple.as_deref() == Some("x86_64-unknown-linux-gnu"))
                .and_then(|entry| entry.public_assets.as_deref()),
            Some(assets_cross.as_path())
        );
    }

    #[test]
    fn managed_public_stylesheet_and_asset_edits_qualify_for_public_assets_only_reload() {
        let (temp, mut config) = fixture(
            None,
            Some("cp styles/input.css public/styles.css && echo ran >> asset-runs.log"),
        );
        let root = temp.path();
        config.paths.public = Some("public".into());
        fs::create_dir_all(root.join("public/images")).unwrap();
        fs::write(root.join("public/styles.css"), ".old { color: red; }").unwrap();
        fs::write(root.join("public/images/brand.svg"), "<svg/>").unwrap();
        fs::write(
            root.join("proa.config.rs"),
            crate::config::generated_rust_config(&config).unwrap(),
        )
        .unwrap();
        let loaded = load_project_config(root).unwrap();
        let mut watch_paths = dev_watch_paths(&loaded, &dev_options(None)).unwrap();

        // 1. Editing an unrelated `public/` asset qualifies for fast republish. The asset
        //    command still runs: its declared input is not proof of everything it reads.
        let committed = watch_snapshot(root, &watch_paths).unwrap();
        fs::write(root.join("public/images/brand.svg"), "<svg id='v2'/>").unwrap();
        let mut failure = committed.clone();
        let asset_only = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(asset_only.public_assets_only);
        assert!(root.join("asset-runs.log").is_file());

        // 2. Editing `styles/input.css` rebuilds CSS and still qualifies for fast catalog republish.
        let committed = asset_only.snapshot;
        fs::write(root.join("styles/input.css"), ".new { color: green; }").unwrap();
        let mut failure = committed.clone();
        let css_edit = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(css_edit.public_assets_only);
        assert_eq!(
            fs::read(root.join("public/styles.css")).unwrap(),
            b".new { color: green; }"
        );
        assert_eq!(
            fs::read_to_string(root.join("asset-runs.log"))
                .unwrap()
                .lines()
                .count(),
            2
        );

        // 3. Deleting a declared asset output (`public/styles.css`) must NOT skip `build_assets`.
        let committed = css_edit.snapshot;
        fs::remove_file(root.join("public/styles.css")).unwrap();
        let mut failure = committed.clone();
        let deleted_css = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(deleted_css.public_assets_only);
        assert_eq!(
            fs::read(root.join("public/styles.css")).unwrap(),
            b".new { color: green; }"
        );
        assert_eq!(
            fs::read_to_string(root.join("asset-runs.log"))
                .unwrap()
                .lines()
                .count(),
            3
        );

        // 4. Editing a Rust source file disables the fast path.
        let committed = deleted_css.snapshot;
        fs::write(root.join("src/main.rs"), "fn main() { /* edited */ }\n").unwrap();
        let mut failure = committed.clone();
        let rust_edit = prepare_dev_inputs(
            root,
            load_project_config(root).unwrap(),
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(!rust_edit.public_assets_only);
    }

    #[test]
    fn compiled_inputs_match_through_a_linked_public_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("site");
        let assets = temp.path().join("shared-assets");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&assets).unwrap();
        fs::write(assets.join("logo.svg"), "one").unwrap();
        fs::write(assets.join("icon.svg"), "one").unwrap();
        std::os::unix::fs::symlink(&assets, root.join("public")).unwrap();
        // rustc records `include_str!("../public/logo.svg")` under the link target.
        let inputs = BTreeSet::from([assets.join("logo.svg").canonicalize().unwrap()]);

        assert!(is_compiled_input(
            &root,
            &inputs,
            Path::new("public/logo.svg")
        ));
        assert!(!is_compiled_input(
            &root,
            &inputs,
            Path::new("public/icon.svg")
        ));
        fs::remove_file(assets.join("logo.svg")).unwrap();
        assert!(is_compiled_input(
            &root,
            &inputs,
            Path::new("public/logo.svg")
        ));
        assert!(is_compiled_input(
            &root,
            &inputs,
            Path::new("gone/logo.svg")
        ));
    }

    #[test]
    fn css_script_reruns_for_public_edits_because_its_inputs_are_undeclared() {
        let (temp, mut config) = fixture(None, None);
        let root = temp.path();
        config.paths.public = Some("public".into());
        config.scripts.css = "cat public/theme.css > public/styles.css".into();
        fs::create_dir(root.join("public")).unwrap();
        fs::write(root.join("public/theme.css"), ".old {}").unwrap();
        fs::write(root.join("public/styles.css"), ".old {}").unwrap();
        fs::write(
            root.join("proa.config.rs"),
            crate::config::generated_rust_config(&config).unwrap(),
        )
        .unwrap();
        let loaded = load_project_config(root).unwrap();
        let mut watch_paths = dev_watch_paths(&loaded, &dev_options(None)).unwrap();
        let committed = watch_snapshot(root, &watch_paths).unwrap();
        fs::write(root.join("public/theme.css"), ".new {}").unwrap();
        let mut failure = committed.clone();
        let edit = prepare_dev_inputs(
            root,
            loaded,
            &dev_options(None),
            &committed,
            None,
            &mut failure,
            &mut watch_paths,
        )
        .unwrap();
        assert!(edit.public_assets_only);
        assert_eq!(
            fs::read(root.join("public/styles.css")).unwrap(),
            b".new {}"
        );
    }
}
