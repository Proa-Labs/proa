// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;

use proa_cli::rsjs::abi_adapter::AdaptedTransportAbi;
use proa_cli::rsjs::build::{
    DescriptorCargoRequest, FINAL_CARGO_UNIT_STATUS_MAPPED, FINAL_WRAPPER_POLICY_VERSION,
};
use proa_cli::rsjs::final_compile::{run_final_cargo, FinalCargoRequest, FINAL_PLAN_FILE_HASH_ENV};
use proa_cli::rsjs::pipeline::{run_linked_cargo, DescriptorRootSelector, LinkedCargoRequest};
use proa_cli::rsjs::semantic_link::{link_semantic_plan, write_content_addressed_semantic_plan};
use rsjs_ir::abi::{AbiVersion, InvocationId, LinkUnitId, ProducerVersion, SemanticHash};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

#[test]
fn linked_cargo_restores_exact_descriptor_identity_across_renamed_targets() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("fixture_macro/src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"rsjs_final_fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [dependencies]\n\
         fixture_macro = { path = \"fixture_macro\" }\n\n\
         [workspace]\n\
         members = [\"fixture_macro\"]\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("fixture_macro/Cargo.toml"),
        "[package]\n\
         name = \"fixture_macro\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [lib]\n\
         proc-macro = true\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("fixture_macro/src/lib.rs"),
        r#"
extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro]
pub fn final_identity(_input: TokenStream) -> TokenStream {
    let identity = format!(
        "{}|{}|{}",
        std::env::var("RSJS_CARGO_UNIT_ID").expect("mapped final Cargo unit"),
        std::env::var("RSJS_FINAL_CARGO_UNIT_STATUS").expect("final unit status"),
        std::env::var("RSJS_LINK_PLAN_FILE_HASH").expect("exact plan hash"),
    );
    format!("{identity:?}").parse().expect("string literal")
}
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]

#[cfg(rsjs_final)]
use fixture_macro::final_identity;

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    if let Some(marker) = std::env::args_os().nth(1) {
        std::fs::write(marker, "cargo-final-executed").unwrap();
    }
    print!("{}", final_identity!());
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
    let package_id = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "rsjs_final_fixture")
        .and_then(|package| package["id"].as_str())
        .unwrap()
        .to_string();

    let generated_root = temp.path().join("target/rsjs-final-fixture");
    let descriptor_target = generated_root.join("descriptor-target-name");
    let request = LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: temp.path().to_path_buf(),
            staging_directory: generated_root.join("descriptor-staging"),
            target_directory: descriptor_target.clone(),
            source_exclusion_directory: generated_root.clone(),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: Default::default(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: "rsjs_final_fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-final-fixture"),
        producer: ProducerVersion::new(0, 1, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: generated_root.join("plans"),
        final_target_root: generated_root.join("renamed-final-target-root"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: Default::default(),
    };

    let first = run_linked_cargo(&request).unwrap();
    assert_eq!(first.final_cargo.plan_id, first.semantic.plan_id);
    assert_ne!(first.final_cargo.target_directory, descriptor_target);
    let invocation_directory = request.invocation.to_string();
    assert_eq!(
        first.final_cargo.target_directory,
        request
            .final_target_root
            .join("rsjs-final")
            .join(format!("wrapper-v{FINAL_WRAPPER_POLICY_VERSION}"))
            .join(&invocation_directory)
    );
    assert_eq!(
        first.final_cargo.target_directory.file_name(),
        Some(std::ffi::OsStr::new(&invocation_directory))
    );
    assert!(first.final_cargo.unit_map_path.is_file());
    assert_eq!(first.final_cargo.unit_map_hash.len(), 64);

    let executable = first
        .final_root_artifact
        .executable
        .as_ref()
        .expect("fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(runtime.status.success());
    assert_eq!(
        String::from_utf8(runtime.stdout).unwrap(),
        format!(
            "{}|{}|{}",
            first.root_unit, FINAL_CARGO_UNIT_STATUS_MAPPED, first.final_cargo.plan_file_hash
        )
    );
    assert_eq!(
        first.final_cargo.plan_file_hash,
        fs::read(&first.semantic_plan_path)
            .map(|bytes| {
                use sha2::{Digest, Sha256};
                let digest: [u8; 32] = Sha256::digest(bytes).into();
                format!(
                    "sha256:{}",
                    digest
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                )
            })
            .unwrap()
    );
    assert_ne!(
        first.final_cargo.plan_file_hash,
        first.semantic.plan_id.to_string()
    );

    assert_eq!(FINAL_PLAN_FILE_HASH_ENV, "RSJS_LINK_PLAN_FILE_HASH");
}

#[test]
fn final_cargo_rebuilds_a_plan_sensitive_consumer_without_rebuilding_unrelated_crates() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("fixture_macro/src")).unwrap();
    fs::create_dir_all(temp.path().join("stable_dependency/src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"rsjs_final_cache_fixture\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [dependencies]\n\
         fixture_macro = { path = \"fixture_macro\" }\n\
         stable_dependency = { path = \"stable_dependency\" }\n\n\
         [workspace]\n\
         members = [\"fixture_macro\", \"stable_dependency\"]\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("fixture_macro/Cargo.toml"),
        "[package]\n\
         name = \"fixture_macro\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\n\
         [lib]\n\
         proc-macro = true\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("fixture_macro/src/lib.rs"),
        r#"
extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro]
pub fn final_identity(_input: TokenStream) -> TokenStream {
    let identity = format!(
        "{}|{}|{}",
        std::env::var("RSJS_PLAN_ID").expect("exact plan ID"),
        std::env::var("RSJS_LINK_PLAN_FILE_HASH").expect("exact plan hash"),
        std::env::var("PROA_RSJS_FINAL_UNIT_MAP_HASH").expect("exact unit-map hash"),
    );
    format!(
        "{{\
            const _: Option<&str> = option_env!(\"RSJS_PLAN_ID\");\
            const _: Option<&str> = option_env!(\"RSJS_LINK_PLAN_FILE_HASH\");\
            const _: Option<&str> = option_env!(\"PROA_RSJS_FINAL_UNIT_MAP_HASH\");\
            {identity:?}\
        }}"
    )
    .parse()
    .expect("tracked final identity expression")
}
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("stable_dependency/Cargo.toml"),
        "[package]\n\
         name = \"stable_dependency\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("stable_dependency/src/lib.rs"),
        "pub const fn value() -> u32 { 17 }\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/main.rs"),
        r#"
#![allow(unexpected_cfgs)]

#[cfg(rsjs_final)]
use fixture_macro::final_identity;

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    print!("{}|{}", stable_dependency::value(), final_identity!());
}
"#,
    )
    .unwrap();

    let empty_abi = AdaptedTransportAbi {
        components: Vec::new(),
        helpers: Vec::new(),
    };
    let plan_a = link_semantic_plan(LinkUnitId::derive("final-cache-plan-a"), &empty_abi).unwrap();
    let plan_b = link_semantic_plan(LinkUnitId::derive("final-cache-plan-b"), &empty_abi).unwrap();
    assert_ne!(plan_a.plan_id, plan_b.plan_id);

    let generated_root = temp.path().join("target/rsjs-final-cache-fixture");
    let plan_directory = generated_root.join("plans");
    let plan_a_path = write_content_addressed_semantic_plan(&plan_directory, &plan_a).unwrap();
    let plan_b_path = write_content_addressed_semantic_plan(&plan_directory, &plan_b).unwrap();
    let request = FinalCargoRequest {
        invocation: InvocationId::derive("final-cache-invocation"),
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        source_exclusion_directory: generated_root.clone(),
        target_root: generated_root.join("final-target"),
        plan_path: plan_a_path,
        cargo_args: vec!["build".into(), "--offline".into()],
        environment: Default::default(),
        root_unit_fingerprint: SemanticHash::derive("final-cache-root-unit"),
        descriptor_units: Vec::new(),
    };

    let output_a = run_final_cargo(&request, &plan_a).unwrap();
    let mut request_b = request.clone();
    request_b.plan_path = plan_b_path;
    let output_b = run_final_cargo(&request_b, &plan_b).unwrap();

    assert_eq!(output_a.target_directory, output_b.target_directory);
    assert_ne!(output_a.plan_file_hash, output_b.plan_file_hash);
    assert_ne!(output_a.unit_map_hash, output_b.unit_map_hash);
    let freshness_b = output_b
        .cargo_artifacts
        .iter()
        .map(|artifact| (artifact.target.name.as_str(), artifact.fresh))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(freshness_b.get("fixture_macro"), Some(&true));
    assert_eq!(freshness_b.get("stable_dependency"), Some(&true));
    assert_eq!(freshness_b.get("rsjs_final_cache_fixture"), Some(&false));

    let output_b_again = run_final_cargo(&request_b, &plan_b).unwrap();
    assert_eq!(output_b.target_directory, output_b_again.target_directory);
    assert!(
        output_b_again
            .cargo_artifacts
            .iter()
            .all(|artifact| artifact.fresh),
        "an unchanged plan-B build must reuse every final Cargo artifact"
    );
}

#[test]
fn semantic_plan_final_input_marks_an_unmapped_non_rsjs_unit_explicitly() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\n\
         name = \"rsjs_unmapped_final_fixture\"\n\
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
compile_error!("semantic final input did not activate the final phase");

fn main() {
    print!(
        "{}|{}|{}",
        env!("RSJS_FINAL_CARGO_UNIT_STATUS"),
        option_env!("RSJS_CARGO_UNIT_ID").unwrap_or("missing"),
        env!("RSJS_LINK_PLAN_FILE_HASH"),
    );
}
"#,
    )
    .unwrap();

    let semantic_plan = link_semantic_plan(
        LinkUnitId::derive("empty-semantic-final"),
        &AdaptedTransportAbi {
            components: Vec::new(),
            helpers: Vec::new(),
        },
    )
    .unwrap();
    let generated_root = temp.path().join("semantic-final-target");
    let plan_path =
        write_content_addressed_semantic_plan(&generated_root.join("plans"), &semantic_plan)
            .unwrap();
    let request = FinalCargoRequest {
        invocation: InvocationId::derive("unmapped-semantic-final"),
        cargo: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
        working_directory: temp.path().to_path_buf(),
        source_exclusion_directory: generated_root.clone(),
        target_root: generated_root.join("final-target"),
        plan_path,
        cargo_args: vec!["build".into(), "--offline".into()],
        environment: Default::default(),
        root_unit_fingerprint: SemanticHash::derive("unmapped-root-unit"),
        descriptor_units: Vec::new(),
    };

    let output = run_final_cargo(&request, &semantic_plan).unwrap();
    let root_report = output
        .cargo_artifacts
        .iter()
        .find(|artifact| artifact.target.name == "rsjs_unmapped_final_fixture")
        .expect("unmapped fixture report");
    assert!(!root_report.fresh);
    assert!(output.plan_file_hash.starts_with("sha256:"));
    assert_eq!(output.plan_file_hash.len(), "sha256:".len() + 64);
}
