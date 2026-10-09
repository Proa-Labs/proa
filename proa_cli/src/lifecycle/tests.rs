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
    let command = build_command(
        &ProjectConfig::default(),
        &BuildOptions {
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
        },
    );

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
    crate::rsjs::pipeline::validate_request(&first.request).unwrap();
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
        first.descriptor_cache_directory,
        source_changed.descriptor_cache_directory
    );
    assert_eq!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&source_changed.request)
    );
    assert!(first.descriptor_cache_directory.ends_with(format!(
        "descriptor-cache-v{DESCRIPTOR_CACHE_SCHEMA_VERSION}"
    )));
    assert!(descriptor_target_dir(&first.request).ends_with(format!(
            "descriptor-target-policy-v{DESCRIPTOR_TARGET_POLICY_VERSION}-sidecar-v{DESCRIPTOR_UNIT_SIDECAR_SCHEMA_VERSION}-persistent-v{PERSISTENT_DESCRIPTOR_TARGET_LAYOUT_VERSION}"
        )));
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
        first.descriptor_cache_directory,
        featured.descriptor_cache_directory
    );
    assert_ne!(
        descriptor_target_dir(&first.request),
        descriptor_target_dir(&featured.request)
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
    crate::rsjs::pipeline::validate_request(&linked_run.request).unwrap();
    assert_eq!(first.request.invocation, linked_run.request.invocation);
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
    crate::rsjs::pipeline::validate_request(&linked_test.request).unwrap();
    assert_ne!(first.request.invocation, linked_test.request.invocation);
    assert_ne!(
        first.descriptor_cache_directory,
        linked_test.descriptor_cache_directory
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
fn missing_tailwind_cli_message_offers_installer_and_homebrew() {
    let message = missing_program_message("tailwindcss");

    assert!(message.contains("--tailwind-only"));
    assert!(message.contains("brew install tailwindcss"));
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
