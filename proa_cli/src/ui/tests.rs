// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn write_project(root: &Path, public: &Path, pro: &Path) {
    fs::create_dir_all(root.join("src/components")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let mut config = ProjectConfig::default();
    config.registries.insert(
        PUBLIC_REGISTRY.to_string(),
        public.join("{name}.json").display().to_string(),
    );
    config.registries.insert(
        PRO_REGISTRY.to_string(),
        pro.join("{name}.json").display().to_string(),
    );
    fs::write(
        root.join(config::CONFIG_FILE),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

fn write_manifest(
    registry_root: &Path,
    name: &str,
    registry: &str,
    tier: &str,
    license: &str,
    target: &str,
    bytes: &[u8],
    dependencies: &[&str],
) {
    fs::create_dir_all(registry_root.join("files")).unwrap();
    let artifact = format!("files/{name}.rs");
    fs::write(registry_root.join(&artifact), bytes).unwrap();
    let hash = sha256_bytes(bytes);
    let dependencies = serde_json::to_value(dependencies).unwrap();
    let manifest = serde_json::json!({
        "$schema": COMPONENT_MANIFEST_SCHEMA,
        "schemaVersion": COMPONENT_MANIFEST_VERSION,
        "name": name,
        "version": "1.0.0",
        "registry": registry,
        "tier": tier,
        "license": license,
        "dependencies": dependencies,
        "files": [{
            "source": artifact,
            "target": target,
            "sha256": hash,
        }],
    });
    fs::write(
        registry_root.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn add_fixture(root: &Path, component: &str) -> Result<AddReport> {
    add(AddOptions {
        root: root.to_path_buf(),
        component: component.to_string(),
        registry: None,
        dry_run: false,
        force: false,
    })
}

fn write_version_dependency_manifest(
    registry_root: &Path,
    name: &str,
    feature: &str,
    component_bytes: &[u8],
) {
    let artifact_root = registry_root.join("artifacts").join(name);
    fs::create_dir_all(&artifact_root).unwrap();
    fs::write(artifact_root.join("component.rs"), component_bytes).unwrap();
    let manifest = serde_json::json!({
        "$schema": COMPONENT_MANIFEST_SCHEMA,
        "schemaVersion": 1,
        "name": name,
        "version": "1.0.0",
        "registry": "@proa",
        "tier": "free",
        "license": "MIT OR Apache-2.0",
        "files": [{
            "source": format!("artifacts/{name}/component.rs"),
            "target": format!("@components/{name}/mod.rs"),
            "sha256": sha256_bytes(component_bytes),
        }],
        "cargoDependencies": [{
            "name": "serde",
            "version": "1",
            "features": [feature],
        }],
    });
    fs::write(
        registry_root.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn hyphenated_component_names_resolve_to_underscored_slugs() {
    // `alert-dialog` is what people type; the registry slug is
    // `alert_dialog` because it doubles as the Cargo feature name.
    assert_eq!(
        canonical_component_reference("alert-dialog"),
        "alert_dialog"
    );
    assert_eq!(canonical_component_reference("button"), "button");
    assert_eq!(
        canonical_component_reference("@proa/radio-group"),
        "@proa/radio_group"
    );
    // The registry alias itself is hyphenated and must survive intact.
    assert_eq!(
        canonical_component_reference("@proa-pro/data-table"),
        "@proa-pro/data_table"
    );
}

#[test]
fn installs_free_component_and_writes_lock() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"pub struct Button;\n",
        &[],
    );

    let report = add_fixture(&project, "button").unwrap();

    assert_eq!(report.resolved, "@proa/button");
    assert_eq!(
        fs::read(project.join("src/components/button/button.rs")).unwrap(),
        b"pub struct Button;\n"
    );
    let lock = config::load_component_lock(&project).unwrap();
    let installed = &lock.components["@proa/button"];
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(
        installed.installed_files,
        [PathBuf::from("src/components/button/button.rs")]
    );
}

#[test]
fn resolves_dependencies_before_committing() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "spinner",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/spinner/spinner.rs",
        b"pub struct Spinner;\n",
        &[],
    );
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"pub struct Button;\n",
        &["spinner"],
    );

    let report = add_fixture(&project, "button").unwrap();

    assert_eq!(report.components.len(), 2);
    assert!(report
        .components
        .iter()
        .any(|component| component.name == "spinner" && component.dependency));
    let lock = config::load_component_lock(&project).unwrap();
    assert_eq!(
        lock.components["@proa/button"].dependencies,
        ["@proa/spinner"]
    );
}

#[test]
fn missing_dependency_leaves_project_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"pub struct Button;\n",
        &["spinner"],
    );

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains("@proa/spinner"), "{error}");
    assert!(!project.join("src/components/button/button.rs").exists());
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn hash_mismatch_leaves_project_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(public.join("files")).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    fs::write(public.join("files/button.rs"), "changed").unwrap();
    let manifest = serde_json::json!({
        "$schema": COMPONENT_MANIFEST_SCHEMA,
        "schemaVersion": 1,
        "name": "button",
        "version": "1.0.0",
        "registry": "@proa",
        "tier": "free",
        "license": "MIT OR Apache-2.0",
        "files": [{
            "source": "files/button.rs",
            "target": "@components/button/button.rs",
            "sha256": sha256_bytes(b"original"),
        }],
    });
    fs::write(
        public.join("button.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains("hash mismatch"), "{error}");
    assert!(!project.join("src/components/button/button.rs").exists());
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn refuses_to_overwrite_local_file() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    let target = project.join("src/components/button/button.rs");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "local customization").unwrap();
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"registry source",
        &[],
    );

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains("refusing to overwrite"), "{error}");
    assert_eq!(fs::read_to_string(target).unwrap(), "local customization");
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn repeat_install_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"pub struct Button;\n",
        &[],
    );

    add_fixture(&project, "button").unwrap();
    let second = add_fixture(&project, "button").unwrap();

    assert!(second.files_written.is_empty());
    assert_eq!(
        second.files_unchanged,
        [PathBuf::from("src/components/button/button.rs")]
    );
}

#[test]
fn byte_identical_glue_is_shared_across_free_and_pro_registries() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    let shared = b"pub const REGISTRY_GLUE: bool = true;\n";
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/registry_glue.rs",
        shared,
        &[],
    );
    write_manifest(
        &pro,
        "calendar",
        PRO_REGISTRY,
        "pro",
        "LicenseRef-Proa-UI-Pro",
        "@components/registry_glue.rs",
        shared,
        &[],
    );

    add_fixture(&project, "button").unwrap();
    let report = add_fixture(&project, "calendar").unwrap();

    assert_eq!(report.registry, PRO_REGISTRY);
    assert!(
        report
            .files_unchanged
            .contains(&PathBuf::from("src/components/registry_glue.rs")),
        "{report:?}"
    );
    let lock = config::load_component_lock(&project).unwrap();
    assert!(lock.components.contains_key("@proa/button"));
    assert!(lock.components.contains_key("@proa-pro/calendar"));
}

#[test]
fn differing_glue_is_not_replaced_during_free_to_pro_install() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/registry_glue.rs",
        b"pub const TIER: &str = \"free\";\n",
        &[],
    );
    write_manifest(
        &pro,
        "calendar",
        PRO_REGISTRY,
        "pro",
        "LicenseRef-Proa-UI-Pro",
        "@components/registry_glue.rs",
        b"pub const TIER: &str = \"pro\";\n",
        &[],
    );

    add_fixture(&project, "button").unwrap();
    let error = add_fixture(&project, "calendar").unwrap_err().to_string();

    assert!(
        error.contains("refusing to overwrite locally different file"),
        "{error}"
    );
    assert_eq!(
        fs::read(project.join("src/components/registry_glue.rs")).unwrap(),
        b"pub const TIER: &str = \"free\";\n"
    );
    let lock = config::load_component_lock(&project).unwrap();
    assert!(lock.components.contains_key("@proa/button"));
    assert!(!lock.components.contains_key("@proa-pro/calendar"));
}

#[test]
fn managed_modules_cannot_mutate_an_installed_immutable_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/mod.rs",
        b"pub mod button;\n",
        &[],
    );
    write_manifest(
        &pro,
        "calendar",
        PRO_REGISTRY,
        "pro",
        "LicenseRef-Proa-UI-Pro",
        "@components/calendar.rs",
        b"pub struct Calendar;\n",
        &[],
    );
    let manifest_path = pro.join("calendar.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["rustModules"] = serde_json::json!([{
        "target": "@components/mod.rs",
        "modules": ["calendar"]
    }]);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    add_fixture(&project, "button").unwrap();
    let error = add_fixture(&project, "calendar").unwrap_err().to_string();

    assert!(
        error.contains("owns it as an immutable artifact"),
        "{error}"
    );
    assert_eq!(
        fs::read(project.join("src/components/mod.rs")).unwrap(),
        b"pub mod button;\n"
    );
    assert!(!project.join("src/components/calendar.rs").exists());
}

#[test]
fn merges_version_dependency_features_across_component_installs() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_version_dependency_manifest(&public, "button", "derive", b"pub struct Button;\n");
    write_version_dependency_manifest(&public, "spinner", "rc", b"pub struct Spinner;\n");

    add_fixture(&project, "button").unwrap();
    add_fixture(&project, "spinner").unwrap();

    let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("serde = { version = \"1\""), "{cargo}");
    assert!(cargo.contains("\"derive\""), "{cargo}");
    assert!(cargo.contains("\"rc\""), "{cargo}");
    let lock = config::load_component_lock(&project).unwrap();
    assert_eq!(
        lock.components["@proa/button"].cargo_dependencies[0].features,
        ["derive"]
    );
    assert_eq!(
        lock.components["@proa/spinner"].cargo_dependencies[0].features,
        ["rc"]
    );
}

#[test]
fn managed_module_block_preserves_user_content() {
    let current = BTreeSet::new();
    let next = BTreeSet::from(["button".to_string()]);
    let first = update_managed_rust_modules(
        Some(b"pub mod local;\n"),
        &current,
        &next,
        Path::new("mod.rs"),
    )
    .unwrap();
    let rendered = String::from_utf8(first.clone()).unwrap();
    assert!(rendered.starts_with("pub mod local;\n"));
    assert!(rendered.contains("pub mod button;\npub use button::*;"));

    let updated = update_managed_rust_modules(
        Some(&first),
        &next,
        &BTreeSet::from(["button".to_string(), "spinner".to_string()]),
        Path::new("mod.rs"),
    )
    .unwrap();
    let rendered = String::from_utf8(updated).unwrap();
    assert!(rendered.contains("pub mod local;"));
    assert!(rendered.contains("pub mod spinner;\npub use spinner::*;"));
}

#[test]
fn plain_slug_falls_back_to_pro_registry_only_after_public_not_found() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &pro,
        "calendar",
        PRO_REGISTRY,
        "pro",
        "LicenseRef-Proa-UI-Pro",
        "@components/calendar/calendar.rs",
        b"pub struct Calendar;\n",
        &[],
    );

    let report = add_fixture(&project, "calendar").unwrap();

    assert_eq!(report.resolved, "@proa-pro/calendar");
    assert_eq!(report.registry, "@proa-pro");
}

#[test]
fn bare_slug_reports_public_and_pro_lookup_failures() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains("public Free registry `@proa`"), "{error}");
    assert!(error.contains(&public.display().to_string()), "{error}");
    assert!(
        error.contains("licensed Pro registry `@proa-pro`"),
        "{error}"
    );
    assert!(error.contains(&pro.display().to_string()), "{error}");
    assert!(
        error.contains("Free registry access does not require a bearer token"),
        "{error}"
    );
    assert!(error.contains("@proa/button"), "{error}");
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn bare_slug_preserves_non_not_found_pro_failure_context() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    fs::write(pro.join("button.json"), b"{").unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains(&public.display().to_string()), "{error}");
    assert!(error.contains("parse component manifest"), "{error}");
    assert!(error.contains(&pro.display().to_string()), "{error}");
    assert!(
        error.contains("Free registry access does not require a bearer token"),
        "{error}"
    );
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn bearer_auth_is_applied_only_to_pro_registry_requests() {
    const SECRET: &str = "test-registry-secret";
    let client = Client::new();
    let public = with_registry_bearer_auth(
        client.get("https://ui.proa.so/r/button.json"),
        PUBLIC_REGISTRY,
        Some(SECRET),
    )
    .build()
    .unwrap();
    assert!(public.headers().get("authorization").is_none());

    let pro = with_registry_bearer_auth(
        client.get("https://console.proa.so/r/calendar.json"),
        PRO_REGISTRY,
        Some(SECRET),
    )
    .build()
    .unwrap();
    assert_eq!(
        pro.headers().get("authorization").unwrap(),
        "Bearer test-registry-secret"
    );
    assert!(!format!("{pro:?}").contains(SECRET));
}

#[test]
fn cargo_credentials_supply_pro_token_after_environment() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("credentials.toml"),
        "[registries.proa]\ntoken = \"file-token\"\n",
    )
    .unwrap();

    assert_eq!(
        pro_registry_bearer_token_from_sources(None, Some(temp.path())).unwrap(),
        Some("file-token".to_string())
    );
    assert_eq!(
        pro_registry_bearer_token_from_sources(Some(" env-token "), Some(temp.path())).unwrap(),
        Some("env-token".to_string())
    );
}

#[test]
fn cargo_legacy_credentials_take_precedence() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("credentials"),
        "[registries.proa]\ntoken = \"legacy-token\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("credentials.toml"),
        "[registries.proa]\ntoken = \"modern-token\"\n",
    )
    .unwrap();

    assert_eq!(
        pro_registry_bearer_token_from_sources(None, Some(temp.path())).unwrap(),
        Some("legacy-token".to_string())
    );
}

#[test]
fn cargo_credentials_parse_errors_do_not_leak_file_contents() {
    const SECRET: &str = "must-not-appear-in-errors";
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("credentials.toml"),
        format!("[registries.proa]\ntoken = \"{SECRET}"),
    )
    .unwrap();

    let error = pro_registry_bearer_token_from_sources(None, Some(temp.path()))
        .unwrap_err()
        .to_string();

    assert!(error.contains("parse Cargo credentials file"), "{error}");
    assert!(!error.contains(SECRET), "{error}");
}

#[test]
fn dry_run_validates_but_writes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/button/button.rs",
        b"pub struct Button;\n",
        &[],
    );

    let report = add(AddOptions {
        root: project.clone(),
        component: "button".into(),
        registry: None,
        dry_run: true,
        force: false,
    })
    .unwrap();

    assert!(report.dry_run);
    assert_eq!(report.files_written.len(), 1);
    assert!(!project.join("src/components/button/button.rs").exists());
    assert!(!project.join(LOCK_FILE).exists());
}

#[test]
fn resolves_only_the_legacy_proa_ui_lib_alias() {
    let temp = tempfile::tempdir().unwrap();
    let config = ProjectConfig::default();

    let (_, relative) = resolve_target(temp.path(), &config, "@lib/proa_ui/src/lib.rs").unwrap();
    assert_eq!(relative, PathBuf::from("src/lib/proa_ui/src/lib.rs"));

    for target in ["@lib/another_crate/src/lib.rs", "@ui/button.rs"] {
        let error = resolve_target(temp.path(), &config, target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported alias"), "{error}");
    }
}

#[test]
fn rejects_traversal_target() {
    let temp = tempfile::tempdir().unwrap();
    let public = temp.path().join("public");
    let pro = temp.path().join("pro");
    fs::create_dir_all(&public).unwrap();
    fs::create_dir_all(&pro).unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    write_project(&project, &public, &pro);
    write_manifest(
        &public,
        "button",
        PUBLIC_REGISTRY,
        "free",
        "MIT OR Apache-2.0",
        "@components/../../outside.rs",
        b"nope",
        &[],
    );

    let error = add_fixture(&project, "button").unwrap_err().to_string();

    assert!(error.contains("unsafe path component"), "{error}");
    assert!(!temp.path().join("outside.rs").exists());
}

#[test]
fn pro_catalog_token_takes_precedence_over_legacy_cargo_token() {
    let token = pro_token_from_environment(|name| match name {
        "PROA_PRO_TOKEN" => Ok("catalog-token".to_string()),
        "CARGO_REGISTRIES_PROA_TOKEN" => {
            panic!("must not read legacy token when catalog token is set")
        }
        _ => unreachable!(),
    })
    .unwrap();
    assert_eq!(token.as_deref(), Some("catalog-token"));
    let token = pro_token_from_environment(|name| match name {
        "PROA_PRO_TOKEN" => Ok(" ".to_string()),
        "CARGO_REGISTRIES_PROA_TOKEN" => Ok("legacy-token".to_string()),
        _ => unreachable!(),
    })
    .unwrap();
    assert_eq!(token.as_deref(), Some("legacy-token"));
    assert!(
        pro_token_from_environment(|_| Err(env::VarError::NotPresent))
            .unwrap()
            .is_none()
    );
}
