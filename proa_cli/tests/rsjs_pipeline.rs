// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use proa_cli::rsjs::build::{DescriptorCargoRequest, FINAL_CARGO_UNIT_STATUS_MAPPED};
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, run_linked_cargo_cached, DescriptorRootSelector, LinkedCargoRequest,
};
use rsjs_ir::abi::{
    AbiVersion, ComponentBindingSource, ComponentEdgeId, ComponentFieldId, ComponentFieldRole,
    ComponentSemanticKey, DescriptorBody, InvocationId, ProducerVersion,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

#[test]
fn descriptor_link_and_final_cargo_run_as_one_exact_pipeline() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"rsjs_pipeline_fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [workspace]\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let mut arguments = std::env::args_os().skip(1);
    if let Some(log) = arguments.next() {
        let forwarded = arguments
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("|");
        std::fs::write(log, forwarded).unwrap();
    }
    print!(
        "{}|{}|{}",
        env!("RSJS_FINAL_CARGO_UNIT_STATUS"),
        env!("RSJS_CARGO_UNIT_ID"),
        env!("RSJS_LINK_PLAN_FILE_HASH"),
    );
}
"#,
    )
    .unwrap();

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(&cargo)
        .current_dir(temp.path())
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package_id = metadata["packages"][0]["id"].as_str().unwrap().to_string();
    let execution_log = temp.path().join("attested-runtime-arguments");
    let staging_directory = temp.path().join("proa-generated/descriptor-staging");

    let request = LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: temp.path().to_path_buf(),
            staging_directory: staging_directory.clone(),
            target_directory: temp.path().join("proa-generated/descriptor-target"),
            source_exclusion_directory: temp.path().join("proa-generated"),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: Default::default(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: "rsjs_pipeline_fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-pipeline-fixture"),
        producer: ProducerVersion::new(0, 1, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec![
            "run".into(),
            "--offline".into(),
            "--".into(),
            execution_log.clone().into_os_string(),
            "alpha".into(),
            "two words".into(),
        ],
        final_environment: Default::default(),
    };
    let output = run_linked_cargo(&request).unwrap();

    assert!(output.transport.components.is_empty());
    assert!(output.semantic.components.is_empty());
    assert!(
        !staging_directory.exists(),
        "a successful uncached linked run must remove its nonce descriptor staging"
    );
    assert_eq!(
        fs::read_to_string(&execution_log).unwrap(),
        "alpha|two words"
    );
    assert!(output
        .semantic_plan_path
        .ends_with(format!("{}.semantic-plan.json", output.semantic.plan_id)));
    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(runtime.status.success());
    let stdout = String::from_utf8(runtime.stdout).unwrap();
    let fields = stdout.split('|').collect::<Vec<_>>();
    assert_eq!(fields[0], FINAL_CARGO_UNIT_STATUS_MAPPED);
    assert_eq!(fields[1], output.root_unit.to_string());
    assert_eq!(fields[2], output.final_cargo.plan_file_hash);

    let failed_staging = temp.path().join("proa-generated/failed-descriptor-staging");
    let mut failed_request = request;
    failed_request.descriptor.staging_directory = failed_staging.clone();
    failed_request.descriptor.target_directory =
        temp.path().join("proa-generated/failed-descriptor-target");
    failed_request.root.package_id = "path+file:///missing#missing@0.0.0".to_string();
    failed_request.invocation = InvocationId::derive("rsjs-pipeline-failed-root");
    failed_request.final_cargo_args = vec!["build".into(), "--offline".into()];
    let error = run_linked_cargo(&failed_request).unwrap_err();
    assert!(matches!(
        error,
        proa_cli::rsjs::pipeline::LinkedCargoError::MissingRoot { .. }
    ));
    assert!(
        failed_staging.is_dir(),
        "a failed linked run must retain its marker-owned descriptor staging"
    );
}

#[test]
fn an_imported_reexported_static_component_links_through_its_rustc_resolved_edge() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"rsjs_static_pipeline_fixture\"\n\
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
        temp.path().join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

mod ui {
    use super::*;

    pub struct StatusBadge {
        pub label: &'static str,
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StatusBadge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <strong>{self.label}</strong>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::StatusBadge as PublicStatusBadge;
    }
}

use ui::public::PublicStatusBadge as ImportedBadge;

struct DocumentationPage {
    title: &'static str,
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocumentationPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>
                {ImportedBadge {
                    label: self.title,
                }}
            </section>
        }
        .render(cx)
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    print!(
        "{}|{}",
        env!("RSJS_CARGO_UNIT_ID"),
        env!("RSJS_LINK_PLAN_FILE_HASH"),
    );
}
"#,
    )
    .unwrap();

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let environment = std::collections::BTreeMap::new();
    let metadata = Command::new(&cargo)
        .current_dir(temp.path())
        .envs(&environment)
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package_id = metadata["packages"][0]["id"].as_str().unwrap().to_string();
    let output = run_linked_cargo(&LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: temp.path().to_path_buf(),
            staging_directory: temp.path().join("proa-generated/descriptor-staging"),
            target_directory: temp.path().join("proa-generated/descriptor-target"),
            source_exclusion_directory: temp.path().join("proa-generated"),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: environment.clone(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: "rsjs_static_pipeline_fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-static-pipeline-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: environment,
    })
    .unwrap();

    assert_eq!(output.transport.components.len(), 2);
    assert_eq!(output.semantic.components.len(), 2);
    let page_key = ComponentSemanticKey::v1(
        "rsjs_static_pipeline_fixture",
        "rsjs_static_pipeline_fixture",
        std::iter::empty::<&str>(),
        "DocumentationPage",
    )
    .unwrap();
    let badge_key = ComponentSemanticKey::v1(
        "rsjs_static_pipeline_fixture",
        "rsjs_static_pipeline_fixture",
        ["ui"],
        "StatusBadge",
    )
    .unwrap();
    let component = output
        .semantic
        .components
        .iter()
        .find(|component| component.semantic_key == page_key)
        .expect("documentation page semantic descriptor");
    assert_eq!(component.semantic_key, page_key);
    let DescriptorBody::Component(descriptor) = &component.descriptor.body else {
        panic!("expected a canonical component descriptor");
    };
    let title = descriptor
        .fields
        .get(&ComponentFieldId::new(0))
        .expect("latent live-capable `title` field");
    assert_eq!(title.source_name, "title");
    assert_eq!(title.role, ComponentFieldRole::Value);
    assert!(title.type_witness.is_some());
    let direct = descriptor
        .direct_components
        .get(&ComponentEdgeId::new(0))
        .expect("rustc-resolved imported component edge");
    assert!(
        matches!(
            direct.bindings.get("label").map(|binding| &binding.source),
            Some(ComponentBindingSource::LiveValue { .. })
        ),
        "the reusable definition must preserve the live pass-through needed \
         when an outer analyzed parent supplies `title`"
    );
    assert!(
        output.semantic.owners.owners.is_empty()
            && output.semantic.owners.candidate_to_owner.is_empty(),
        "a static root supplies no browser origin, so its latent live chain \
         must still produce no final owner"
    );
    assert_eq!(component.component_edges.len(), 1);
    assert_eq!(component.component_edges[0].target, badge_key);
    assert_eq!(
        component.component_edges[0].target_definition,
        output
            .semantic
            .components
            .iter()
            .find(|candidate| candidate.semantic_key == badge_key)
            .expect("status badge semantic descriptor")
            .definition
    );
    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final static fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(runtime.status.success());
    assert_eq!(
        String::from_utf8(runtime.stdout).unwrap(),
        format!("{}|{}", output.root_unit, output.final_cargo.plan_file_hash)
    );
}

#[test]
fn cached_build_script_component_survives_a_second_source_edit_pipeline() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"rsjs_build_script_pipeline_fixture\"\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\
             build = \"build.rs\"\n\n\
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
        temp.path().join("build.rs"),
        r#"
fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mode = std::env::var("GEN_MODE").unwrap();
    let generated_label = match mode.as_str() {
        "v1" => "from-build-script-v1",
        "v2" => "from-build-script-v2",
        other => panic!("unexpected GEN_MODE {other}"),
    };
    std::fs::write(
        out.join("generated.rs"),
        format!("pub const GENERATED_LABEL: &str = {generated_label:?};\n"),
    )
    .unwrap();
    if mode == "v1" {
        std::fs::write(out.join("obsolete.txt"), "stale history\n").unwrap();
    }
    println!("cargo::rustc-env=BUILD_SCRIPT_MODE=stable");
    println!("cargo::rerun-if-env-changed=GEN_MODE");
    println!("cargo::rerun-if-changed=build.rs");
}
"#,
    )
    .unwrap();

    let source = temp.path().join("src/main.rs");
    let source_text = |revision: &str| {
        format!(
            r#"
#![allow(unexpected_cfgs)]

use proa_core::{{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError}};
use rsjs::rsjs;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));
const SOURCE_REVISION: &str = "{revision}";
const BUILD_MODE: &str = env!("BUILD_SCRIPT_MODE");

struct BuildScriptPage;

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BuildScriptPage {{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {{
        let source_revision = SOURCE_REVISION;
        let generated_label = GENERATED_LABEL;
        proa_macros::html_sync! {{
            <main data-revision={{source_revision}}>{{generated_label}}</main>
        }}
        .render(cx)
    }}
}}

#[cfg(not(rsjs_final))]
fn main() {{}}

#[cfg(rsjs_final)]
fn main() {{
    let _component = BuildScriptPage;
    print!("{{}}|{{}}|{{}}", SOURCE_REVISION, GENERATED_LABEL, BUILD_MODE);
}}
"#
        )
    };
    fs::write(&source, source_text("same-source")).unwrap();

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let environment = std::collections::BTreeMap::new();
    let metadata = Command::new(&cargo)
        .current_dir(temp.path())
        .envs(&environment)
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package_id = metadata["packages"][0]["id"].as_str().unwrap().to_string();
    let make_request = |staging: &str, mode: &str| {
        let mut environment = environment.clone();
        environment.insert("GEN_MODE".into(), mode.into());
        LinkedCargoRequest {
            descriptor: DescriptorCargoRequest {
                cargo: cargo.clone(),
                wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
                working_directory: temp.path().to_path_buf(),
                staging_directory: temp.path().join("proa-generated").join(staging),
                target_directory: temp.path().join("proa-generated/descriptor-target"),
                source_exclusion_directory: temp.path().join("proa-generated"),
                cargo_args: vec!["build".into(), "--offline".into()],
                environment: environment.clone(),
            },
            root: DescriptorRootSelector {
                package_id: package_id.clone(),
                target_name: "rsjs_build_script_pipeline_fixture".to_string(),
                target_kinds: BTreeSet::from(["bin".to_string()]),
                role: ArtifactRole::RootExecutable,
                artifact_path: None,
            },
            invocation: InvocationId::derive("rsjs-build-script-pipeline-fixture"),
            producer: ProducerVersion::new(0, 2, 0),
            runtime_protocol: AbiVersion::new(1, 0),
            plan_directory: temp.path().join("proa-generated/plans"),
            final_target_root: temp.path().join("proa-generated/final-target"),
            final_cargo_args: vec!["build".into(), "--offline".into()],
            final_environment: environment,
        }
    };
    let cache = temp.path().join("proa-generated/descriptor-cache");
    let first =
        run_linked_cargo_cached(&make_request("descriptor-staging-1", "v1"), &cache).unwrap();

    let second =
        run_linked_cargo_cached(&make_request("descriptor-staging-2", "v2"), &cache).unwrap();

    assert!(
        walkdir::WalkDir::new(temp.path().join("proa-generated/descriptor-target"))
            .into_iter()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == "obsolete.txt"),
        "Cargo documents that persistent OUT_DIRs retain obsolete files; the fixture must exercise that history"
    );

    assert_eq!(first.root_unit, second.root_unit);
    assert_ne!(
        first.final_cargo.unit_map_hash,
        second.final_cargo.unit_map_hash
    );
    assert_eq!(
        first.final_cargo.target_directory,
        second.final_cargo.target_directory
    );
    assert_eq!(first.semantic.components.len(), 1);
    assert_eq!(second.semantic.components.len(), 1);
    let first_executable = first
        .final_root_artifact
        .executable
        .as_ref()
        .expect("first immutable final root executable");
    let executable = second
        .final_root_artifact
        .executable
        .as_ref()
        .expect("second final build-script fixture executable");
    assert_ne!(first_executable, executable);
    let first_runtime = Command::new(first_executable).output().unwrap();
    assert!(first_runtime.status.success());
    assert_eq!(
        String::from_utf8(first_runtime.stdout).unwrap(),
        "same-source|from-build-script-v1|stable"
    );
    let runtime = Command::new(executable).output().unwrap();
    assert!(runtime.status.success());
    assert_eq!(
        String::from_utf8(runtime.stdout).unwrap(),
        "same-source|from-build-script-v2|stable"
    );
}
