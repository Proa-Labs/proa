// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end regression coverage for a wide dynamic handler range whose
//! induction variable crosses a standalone-helper boundary.
//!
//! The source frontend proves the range as `i64`. That proof must survive HIR,
//! Client IR JSON, semantic linking, and final fragment emission; defaulting the
//! induction variable to `i32` in any later phase makes this valid Rust program
//! fail during final code generation.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{run_linked_cargo, DescriptorRootSelector, LinkedCargoRequest};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, HelperSemanticKey, InvocationId, ProducerVersion,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

const APP_PACKAGE: &str = "rsjs_linked_helper_range_codegen_fixture";

const APP_SOURCE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{rsjs, signal};

#[rsjs::client]
fn wide_identity(value: i64) -> i64 {
    value
}

struct WideRangeHelper;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for WideRangeHelper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let current = signal(0_i64);
        proa_macros::html_sync! {
            <main data-wide-range-helper>
                <button on_click={|_| {
                    for index in current.get()..3_i64 {
                        current.set(wide_identity(index));
                    }
                }}>
                    "Apply"
                </button>
                <output>{current.get()}</output>
            </main>
        }
        .render(cx)
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let html = proa_core::render_to_string(WideRangeHelper);
    let manifest = rsjs::manifest::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "WideRangeHelper")
        .expect("WideRangeHelper final component fragment");
    let js = fragment.js_source;

    assert!(!js.contains("__rsjs_unlinked_"), "{js}");
    assert_eq!(js.matches("function __rsjs_helper_").count(), 1, "{js}");
    assert!(
        js.contains("const __rsjs_range_end_raw_0 = 3n;"),
        "the i64 range end must remain BigInt-backed:\n{js}"
    );
    assert!(
        js.contains(
            "for (let __local_0 = __rsjs_range_start_0; __local_0 < __rsjs_range_end_0; __local_0 += typeof __local_0 === \"bigint\" ? 1n : 1) {"
        ),
        "the i64 induction variable must use BigInt-preserving iteration:\n{js}"
    );
    assert!(js.contains("__rsjs_helper_0(__local_0)"), "{js}");
    print!("{html}");
}
"#;

#[test]
fn final_codegen_preserves_wide_range_domain_at_helper_call() {
    let temp = write_fixture();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let environment = BTreeMap::new();
    let package_id = fixture_package_id(temp.path(), &cargo, &environment);

    let output = run_linked_cargo(&LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: temp.path().to_path_buf(),
            staging_directory: temp.path().join("proa-generated/descriptor-staging"),
            target_directory: temp.path().join("proa-generated/descriptor-target"),
            source_exclusion_directory: temp.path().join("proa-generated"),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: environment.clone(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: APP_PACKAGE.to_owned(),
            target_kinds: BTreeSet::from(["bin".to_owned()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-linked-helper-range-codegen-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: environment,
    })
    .expect("the wide-range helper fixture must link and compile");

    let component_key = ComponentSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        std::iter::empty::<&str>(),
        "WideRangeHelper",
    )
    .unwrap();
    let helper_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        std::iter::empty::<&str>(),
        "wide_identity",
    )
    .unwrap();
    let component = output
        .semantic
        .components
        .iter()
        .find(|component| component.semantic_key == component_key)
        .expect("WideRangeHelper semantic component");
    assert_eq!(component.helper_edges.len(), 1);
    assert_eq!(component.helper_edges[0].target, helper_key);
    assert_eq!(component.helper_closure, vec![helper_key.clone()]);
    assert_eq!(output.semantic.reachable_helpers, vec![helper_key]);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final wide-range helper executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "final wide-range helper executable failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&runtime.stdout),
        String::from_utf8_lossy(&runtime.stderr),
    );
    assert!(
        String::from_utf8_lossy(&runtime.stdout).contains("data-wide-range-helper"),
        "{}",
        String::from_utf8_lossy(&runtime.stdout),
    );
}

fn write_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = {APP_PACKAGE:?}\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
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
    fs::write(temp.path().join("src/main.rs"), APP_SOURCE).unwrap();
    temp
}

fn fixture_package_id(
    fixture: &Path,
    cargo: &OsString,
    environment: &BTreeMap<OsString, OsString>,
) -> String {
    let metadata = Command::new(cargo)
        .current_dir(fixture)
        .envs(environment)
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == APP_PACKAGE)
        .and_then(|package| package["id"].as_str())
        .expect("fixture root package id")
        .to_owned()
}
