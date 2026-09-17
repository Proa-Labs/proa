// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::Cursor;
use std::process::Command;

use proa_cli::rsjs::abi_adapter::{
    adapt_transport_plan, AbiAdapterContext, AbiBridgeResult, CompatibilityGap,
};
use proa_cli::rsjs::build::{
    descriptor_run_report, load_descriptor_unit_sidecars, run_descriptor_cargo,
    CargoArtifactCollector, DescriptorCargoReport, DescriptorCargoRequest, DescriptorCargoRunError,
    CARGO_UNIT_ID_ENV, DESCRIPTOR_STAGING_DIR_ENV, DESCRIPTOR_TARGET_DIR_ENV,
    SOURCE_EXCLUSION_DIR_ENV, WRAPPER_MODE_DESCRIPTOR_V1, WRAPPER_MODE_ENV,
};
use proa_cli::rsjs::link::write_content_addressed_transport_plan;
use proa_cli::rsjs::pipeline::{link_descriptor_transport, DescriptorRootSelector};
use rsjs_ir::abi::{AbiVersion, InvocationId, ProducerVersion};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

fn rustc_program() -> OsString {
    std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"))
}

fn root_selector(
    report: &DescriptorCargoReport,
    target_name: &str,
    role: ArtifactRole,
) -> DescriptorRootSelector {
    let matches = report
        .cargo_artifacts
        .iter()
        .filter(|artifact| artifact.target_name == target_name)
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "fixture must publish exactly one Cargo artifact named `{target_name}`"
    );
    let artifact = matches[0];
    DescriptorRootSelector {
        package_id: artifact.package_id.clone(),
        target_name: artifact.target_name.clone(),
        target_kinds: artifact.target_kinds.clone(),
        role,
        artifact_path: None,
    }
}

#[test]
fn noncached_runner_rejects_parent_components_that_escape_source_exclusion() {
    let temp = TempDir::new().unwrap();
    let generated = temp.path().join("generated");
    let error = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: OsString::from("cargo"),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: generated.join("staging"),
        // A sibling of the exclusion directory: absolute and free of `..`
        // components, so it passes the AmbiguousManagedDirectory pre-check and
        // exercises the outside-the-exclusion rejection this test is about.
        target_directory: temp.path().join("outside/descriptor"),
        source_exclusion_directory: generated.clone(),
        cargo_args: vec![OsString::from("check")],
        environment: Default::default(),
    })
    .unwrap_err();

    assert!(matches!(
        error,
        DescriptorCargoRunError::TargetOutsideSourceExclusion { .. }
    ));
    assert!(!generated.join("staging").exists());
}

#[test]
fn noncached_runner_rejects_staging_that_can_contaminate_source_identity() {
    let temp = TempDir::new().unwrap();
    let generated = temp.path().join("generated");
    let staging = temp.path().join("package-local-staging");
    let error = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: OsString::from("cargo"),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: staging.clone(),
        target_directory: generated.join("descriptor"),
        source_exclusion_directory: generated,
        cargo_args: vec![OsString::from("check")],
        environment: Default::default(),
    })
    .unwrap_err();

    assert!(matches!(
        error,
        DescriptorCargoRunError::StagingOutsideSourceExclusion { .. }
    ));
    assert!(!staging.exists());
}

#[test]
fn hidden_wrapper_emits_one_exact_object_and_atomic_unit_sidecar() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("lib.rs");
    let target = temp.path().join("descriptor-target");
    let output = target.join("debug/deps");
    let staging = temp.path().join("explicit-staging");
    fs::create_dir_all(&output).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::write(
        &source,
        format!(
            "pub const UNIT: &str = env!(\"{CARGO_UNIT_ID_ENV}\");\n\
             pub const SECRET: Option<&str> = option_env!(\"PROA_WRAPPER_SECRET\");\n\
             pub fn unit() -> &'static str {{ UNIT }}\n"
        ),
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
        .env(WRAPPER_MODE_ENV, WRAPPER_MODE_DESCRIPTOR_V1)
        .env(DESCRIPTOR_STAGING_DIR_ENV, &staging)
        .env(DESCRIPTOR_TARGET_DIR_ENV, &target)
        .env(SOURCE_EXCLUSION_DIR_ENV, &target)
        .env("CARGO_MANIFEST_DIR", temp.path())
        .env("PROA_WRAPPER_SECRET", "do-not-persist-this-value")
        .args([
            rustc_program(),
            "--crate-name".into(),
            "wrapper_fixture".into(),
            "--crate-type=lib".into(),
            "--edition=2021".into(),
            "--emit=dep-info,metadata,link".into(),
            "--print=link-args".into(),
            "--out-dir".into(),
            output.as_os_str().to_owned(),
            source.as_os_str().to_owned(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let sidecars = fs::read_dir(staging.join("units"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(sidecars.len(), 1);
    let sidecar_bytes = fs::read(&sidecars[0]).unwrap();
    assert!(!String::from_utf8_lossy(&sidecar_bytes).contains("do-not-persist-this-value"));
    let sidecar: serde_json::Value = serde_json::from_slice(&sidecar_bytes).unwrap();
    let unit = sidecar["cargo_unit_id"].as_str().unwrap();
    assert_eq!(unit.len(), 64);
    assert!(unit.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        sidecars[0].file_name().unwrap().to_string_lossy(),
        format!("{unit}.json")
    );

    let relative_object = sidecar["descriptor_object"].as_str().unwrap();
    assert_eq!(relative_object, format!("objects/{unit}.o"));
    let exact_object = staging.join(relative_object);
    assert!(exact_object.is_file());
    assert!(fs::metadata(exact_object).unwrap().len() > 0);
    assert_eq!(sidecar["normalized_rustc_inputs_hash"].as_str(), Some(unit));
    assert_eq!(
        sidecar["environment_dependencies"][0]["name"].as_str(),
        Some("PROA_WRAPPER_SECRET")
    );
    assert!(sidecar["environment_dependencies"][0]["value_hash"]
        .as_str()
        .is_some_and(|hash| hash.len() == 64));
    assert!(sidecar["source_dependencies"]
        .as_array()
        .is_some_and(|dependencies| !dependencies.is_empty()));

    let validated = load_descriptor_unit_sidecars(&staging).unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(validated[0].unit.to_string(), unit);
    assert_eq!(
        validated[0].unit.as_abi_id().as_bytes(),
        validated[0].unit.as_bytes()
    );
}

#[test]
fn rustc_probes_are_forwarded_without_creating_unit_outputs() {
    let temp = TempDir::new().unwrap();
    let staging = temp.path().join("explicit-staging");
    let target = temp.path().join("descriptor-target");
    fs::create_dir_all(&target).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-proa"))
        .env(WRAPPER_MODE_ENV, WRAPPER_MODE_DESCRIPTOR_V1)
        .env(DESCRIPTOR_STAGING_DIR_ENV, &staging)
        .env(DESCRIPTOR_TARGET_DIR_ENV, &target)
        .env(SOURCE_EXCLUSION_DIR_ENV, &target)
        .args([rustc_program(), OsString::from("-vV")])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with("rustc "));
    assert!(!staging.exists());
}

#[cfg(unix)]
#[test]
fn cargo_configured_workspace_wrapper_fails_with_an_explicit_diagnostic() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("src");
    let cargo_config_dir = temp.path().join(".cargo");
    let workspace_wrapper = temp.path().join("workspace-rustc-wrapper.sh");
    let target = temp.path().join("generated/descriptor-target");
    let staging = temp.path().join("generated/descriptor-staging");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&cargo_config_dir).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"composed_wrapper_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(source_dir.join("lib.rs"), "pub fn fixture() {}\n").unwrap();
    fs::write(&workspace_wrapper, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    let mut permissions = fs::metadata(&workspace_wrapper).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&workspace_wrapper, permissions).unwrap();
    fs::write(
        cargo_config_dir.join("config.toml"),
        format!(
            "[build]\nrustc-workspace-wrapper = {}\n",
            toml::Value::String(workspace_wrapper.to_string_lossy().into_owned())
        ),
    )
    .unwrap();

    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(temp.path())
        .args([
            OsString::from("check"),
            OsString::from("--manifest-path"),
            temp.path().join("Cargo.toml").into_os_string(),
            OsString::from("--target-dir"),
            target.clone().into_os_string(),
            OsString::from("--offline"),
        ])
        .env("RUSTC_WRAPPER", env!("CARGO_BIN_EXE_cargo-proa"))
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env(WRAPPER_MODE_ENV, WRAPPER_MODE_DESCRIPTOR_V1)
        .env(DESCRIPTOR_STAGING_DIR_ENV, &staging)
        .env(DESCRIPTOR_TARGET_DIR_ENV, &target)
        .env(SOURCE_EXCLUSION_DIR_ENV, temp.path().join("generated"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("build.rustc-workspace-wrapper"),
        "unexpected Cargo diagnostic:\n{stderr}"
    );
}

#[test]
fn collector_accepts_live_cargo_json_and_uses_its_exact_paths() {
    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"cargo_json_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(source_dir.join("lib.rs"), "pub fn fixture() {}\n").unwrap();

    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "check",
            "--manifest-path",
            temp.path().join("Cargo.toml").to_str().unwrap(),
            "--target-dir",
            temp.path().join("target").to_str().unwrap(),
            "--message-format=json",
            "--offline",
        ])
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove(WRAPPER_MODE_ENV)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut collector = CargoArtifactCollector::default();
    collector.ingest_reader(Cursor::new(output.stdout)).unwrap();
    let artifacts = collector.finish().unwrap();
    let fixture = artifacts
        .iter()
        .find(|artifact| artifact.target.name == "cargo_json_fixture")
        .unwrap();
    assert!(fixture
        .filenames
        .iter()
        .all(|path| path.starts_with(temp.path().join("target"))));
    assert!(fixture.filenames.iter().all(|path| path.is_absolute()));
}

#[test]
fn descriptor_check_runner_emits_an_object_and_collects_exact_cargo_outputs() {
    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    let manifest = temp.path().join("Cargo.toml");
    fs::write(
        &manifest,
        "[package]\nname = \"descriptor_runner_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(source_dir.join("lib.rs"), "pub fn fixture() {}\n").unwrap();

    let run = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: temp.path().join("target/descriptor-staging"),
        target_directory: temp.path().join("target/descriptor"),
        source_exclusion_directory: temp.path().join("target"),
        cargo_args: vec![
            "check".into(),
            "--manifest-path".into(),
            manifest.clone().into_os_string(),
            "--offline".into(),
        ],
        environment: Default::default(),
    })
    .unwrap();

    let report = descriptor_run_report(&run);
    assert!(report
        .cargo_artifacts
        .iter()
        .any(|artifact| artifact.target_name == "descriptor_runner_fixture"));
    assert_eq!(report.units.len(), 1);
    assert!(
        report.units[0].descriptor_object_hash.len() == 64,
        "the path-free report must retain the exact descriptor-object identity"
    );
    assert_eq!(
        report.units[0].cargo_package_name.as_deref(),
        Some("descriptor_runner_fixture")
    );

    let selector = root_selector(
        &report,
        "descriptor_runner_fixture",
        ArtifactRole::RootLibrary,
    );
    let linked = link_descriptor_transport(
        run,
        InvocationId::derive("descriptor-runner-fixture"),
        &selector,
    )
    .unwrap();
    assert_eq!(linked.descriptor, report);
    assert_eq!(linked.root_unit, report.units[0].unit);
    assert!(linked.transport.components.is_empty());
    assert!(linked.transport.helpers.is_empty());
}

#[test]
fn linked_descriptor_transport_accepts_the_exact_reachable_extern_graph() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"dep\", \"app\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    for package in ["dep", "app"] {
        fs::create_dir_all(temp.path().join(package).join("src")).unwrap();
    }
    fs::write(
        temp.path().join("dep/Cargo.toml"),
        "[package]\nname = \"descriptor_graph_dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("dep/src/lib.rs"),
        "pub fn dependency() -> u32 { 7 }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("app/Cargo.toml"),
        "[package]\nname = \"descriptor_graph_app\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
         [dependencies]\ndescriptor_graph_dep = { path = \"../dep\" }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("app/src/lib.rs"),
        "pub fn app() -> u32 { descriptor_graph_dep::dependency() }\n",
    )
    .unwrap();

    let run = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: temp.path().join("target/descriptor-staging"),
        target_directory: temp.path().join("target/descriptor"),
        source_exclusion_directory: temp.path().join("target"),
        cargo_args: vec![
            "check".into(),
            "-p".into(),
            "descriptor_graph_app".into(),
            "--offline".into(),
        ],
        environment: Default::default(),
    })
    .unwrap();
    let report = descriptor_run_report(&run);
    let root = report
        .units
        .iter()
        .find(|unit| unit.cargo_package_name.as_deref() == Some("descriptor_graph_app"))
        .unwrap();
    let root_unit = root.unit;
    let package_names = report
        .units
        .iter()
        .filter_map(|unit| unit.cargo_package_name.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        package_names,
        std::collections::BTreeSet::from(["descriptor_graph_app", "descriptor_graph_dep"])
    );
    let selector = root_selector(&report, "descriptor_graph_app", ArtifactRole::RootLibrary);
    let linked = link_descriptor_transport(
        run,
        InvocationId::derive("descriptor-graph-fixture"),
        &selector,
    )
    .unwrap();

    assert_eq!(linked.descriptor, report);
    assert_eq!(linked.root_unit, root_unit);
    assert!(linked.transport.components.is_empty());
    assert!(linked.transport.helpers.is_empty());
}

#[test]
fn exact_manifest_links_embedded_components_and_helpers_into_a_plan() {
    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"descriptor_link_fixture\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n\n\
             [dependencies]\n\
             proa_core = {{ path = {:?} }}\n\
             proa_macros = {{ path = {:?} }}\n\
             rsjs = {{ path = {:?} }}\n",
            proa_root.join("proa_core"),
            proa_root.join("proa_macros"),
            proa_root.join("rsjs"),
        ),
    )
    .unwrap();
    fs::write(
        source_dir.join("lib.rs"),
        r#"
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{rsjs, signal};

#[rsjs::client]
fn doubled(value: i32) -> i32 {
    value * 2
}

struct Badge {
    value: i32,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Badge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <strong>{self.value}</strong>
        }
        .render(cx)
    }
}

#[allow(dead_code)]
struct Dashboard;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Dashboard {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(2_i32);
        proa_macros::html_sync! {
            <main>
                {Badge { value: 2_i32 }}
                <output>{doubled(count.get())}</output>
            </main>
        }
        .render(cx)
    }
}
"#,
    )
    .unwrap();

    let run = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: temp.path().join("target/descriptor-staging"),
        target_directory: temp.path().join("target/descriptor"),
        source_exclusion_directory: temp.path().join("target"),
        cargo_args: vec![
            "check".into(),
            "--manifest-path".into(),
            temp.path().join("Cargo.toml").into_os_string(),
            "--offline".into(),
        ],
        environment: std::collections::BTreeMap::new(),
    })
    .unwrap();
    let report = descriptor_run_report(&run);
    let selector = root_selector(
        &report,
        "descriptor_link_fixture",
        ArtifactRole::RootLibrary,
    );
    let linked = link_descriptor_transport(
        run,
        InvocationId::derive("descriptor-link-fixture"),
        &selector,
    )
    .unwrap();

    assert_eq!(linked.descriptor, report);
    let plan = linked.transport;
    assert_eq!(plan.components.len(), 2);
    assert_eq!(plan.helpers.len(), 1);
    assert_eq!(
        plan.components
            .iter()
            .map(|component| component.component_edges.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(
        plan.components
            .iter()
            .map(|component| component.helper_edges.len())
            .sum::<usize>(),
        1
    );
    let component_items = plan
        .components
        .iter()
        .map(|component| component.identity.item.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        component_items,
        std::collections::BTreeSet::from(["Badge", "Dashboard"])
    );
    assert!(plan.components.iter().all(|component| {
        component.identity.package == "descriptor_link_fixture"
            && component.identity.crate_name == "descriptor_link_fixture"
            && component.identity.module_path.is_empty()
    }));
    assert_eq!(plan.helpers[0].identity.item, "doubled");
    assert_eq!(plan.helpers[0].identity.package, "descriptor_link_fixture");
    assert!(plan.helpers[0].identity.module_path.is_empty());

    let adapter = AbiAdapterContext::from_transport_plan(
        ProducerVersion::new(0, 2, 0),
        AbiVersion::new(1, 0),
        &plan,
    )
    .unwrap();
    let adapted = adapt_transport_plan(&adapter, &plan).unwrap();
    for result in adapted.components.iter().chain(&adapted.helpers) {
        if let AbiBridgeResult::Unsupported(report) = result {
            assert!(
                !report
                    .gaps
                    .contains(&CompatibilityGap::MissingResolvedSemanticIdentity),
                "exact identity transport must populate the adapter context: {report}"
            );
        }
    }

    let plans = temp.path().join("plans");
    let first = write_content_addressed_transport_plan(&plans, &plan).unwrap();
    let second = write_content_addressed_transport_plan(&plans, &plan).unwrap();
    assert_eq!(first, second);
    let written: serde_json::Value = serde_json::from_slice(&fs::read(first).unwrap()).unwrap();
    assert_eq!(written["plan_id"], plan.plan_id.to_string());
    assert_eq!(written["link_unit_id"], plan.link_unit.to_string());
}

#[test]
fn exact_manifest_rejects_a_reachable_unannotated_helper() {
    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("src");
    fs::create_dir_all(&source_dir).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"unannotated_helper_link_fixture\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n\n\
             [dependencies]\n\
             proa_core = {{ path = {:?} }}\n\
             proa_macros = {{ path = {:?} }}\n\
             rsjs = {{ path = {:?} }}\n",
            proa_root.join("proa_core"),
            proa_root.join("proa_macros"),
            proa_root.join("rsjs"),
        ),
    )
    .unwrap();
    fs::write(
        source_dir.join("lib.rs"),
        r#"
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{rsjs, signal};

fn ordinary_double(value: i32) -> i32 {
    value * 2
}

struct Counter;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(2_i32);
        proa_macros::html_sync! {
            <output>{ordinary_double(count.get())}</output>
        }
        .render(cx)
    }
}
"#,
    )
    .unwrap();

    let run = run_descriptor_cargo(&DescriptorCargoRequest {
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        staging_directory: temp.path().join("target/descriptor-staging"),
        target_directory: temp.path().join("target/descriptor"),
        source_exclusion_directory: temp.path().join("target"),
        cargo_args: vec![
            "check".into(),
            "--manifest-path".into(),
            temp.path().join("Cargo.toml").into_os_string(),
            "--offline".into(),
        ],
        environment: std::collections::BTreeMap::new(),
    })
    .unwrap();
    let report = descriptor_run_report(&run);
    let selector = root_selector(
        &report,
        "unannotated_helper_link_fixture",
        ArtifactRole::RootLibrary,
    );
    let error = link_descriptor_transport(
        run,
        InvocationId::derive("unannotated-helper-link-fixture"),
        &selector,
    )
    .expect_err("an ordinary Rust function cannot satisfy a reachable client-helper edge");
    assert!(
        error
            .to_string()
            .contains("unresolved RSJS client-helper function"),
        "the link-stage diagnostic should identify the missing `#[rsjs::client]` definition: \
         {error}"
    );
}
