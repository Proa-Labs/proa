// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for signature-directed integer literals at
//! the standalone-helper boundary.
//!
//! Unsuffixed Rust literals are contextually typed by the callee, but the
//! descriptor phase cannot resolve an imported helper signature and therefore
//! transports them as generic `Value::Int` leaves. This fixture proves the
//! final rustc-authenticated link restores i64/u64 BigInt representation for
//! root calls, transitive helper calls, and Option/Vec-nested literal leaves.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{run_linked_cargo, DescriptorRootSelector, LinkedCargoRequest};
use rsjs_ir::abi::{AbiVersion, InvocationId, ProducerVersion};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

const APP_PACKAGE: &str = "rsjs_linked_helper_wide_literal_fixture";

const APP_SOURCE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use std::fs;

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{rsjs, signal};

#[rsjs::client]
fn signed_identity(_tick: i32, value: i64) -> i64 {
    value
}

#[rsjs::client]
fn unsigned_identity(_tick: i32, value: u64) -> u64 {
    value
}

#[rsjs::client]
fn nested_present(_tick: i32, value: Option<Vec<Option<i64>>>) -> i32 {
    if value.is_some() { 1 } else { 0 }
}

#[rsjs::client]
fn transitive_literal(tick: i32) -> i64 {
    signed_identity(tick, 9_007_199_254_740_999)
}

struct WideLiteralProbe;

#[rsjs(component, client)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for WideLiteralProbe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let tick = signal(0_i32);
        proa_macros::html_sync! {
            <main data-wide-literal-probe>
                <output>{signed_identity(tick.get(), 9_007_199_254_740_993)}</output>
                <output>{signed_identity(tick.get(), -9_007_199_254_740_991)}</output>
                <output>{unsigned_identity(tick.get(), 9_007_199_254_740_995)}</output>
                <output>{nested_present(tick.get(), Some(vec![Some(9_007_199_254_740_997), None]))}</output>
                <output>{transitive_literal(tick.get())}</output>
            </main>
        }
        .render(cx)
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let html = proa_core::render_to_string(WideLiteralProbe);
    assert_eq!(html.matches("<output").count(), 5, "{html}");

    let manifest = rsjs::manifest::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "WideLiteralProbe")
        .expect("WideLiteralProbe final component fragment");
    let js = fragment.js_source;
    assert!(!js.contains("__rsjs_unlinked_"), "{js}");
    for literal in [
        "9007199254740993n",
        "-9007199254740991n",
        "9007199254740995n",
        "9007199254740997n",
        "9007199254740999n",
    ] {
        assert!(
            js.contains(literal),
            "contextual wide literal `{literal}` was not emitted as a BigInt:\n{js}"
        );
    }
    assert!(
        js.contains("[9007199254740997n, null]"),
        "Option/Vec-nested literal leaves lost their recursive parameter context:\n{js}"
    );

    if let Some(directory) = std::env::var_os("RSJS_NODE_DUMP_DIR") {
        let directory = std::path::PathBuf::from(directory);
        fs::create_dir_all(&directory).expect("create wide-literal Node acceptance directory");
        fs::write(directory.join("package.json"), "{\"type\":\"module\"}\n")
            .expect("write Node package marker");
        fs::write(directory.join("export-name.txt"), fragment.export_name)
            .expect("write exact fragment export name");
        fs::write(
            directory.join("component.js"),
            fragment
                .js_source
                .replace("from \"/rsjs-runtime.js\"", "from \"./rsjs-runtime.js\""),
        )
        .expect("write exact final wide-literal fragment");
    }

    print!("{html}");
}
"#;

#[test]
fn final_codegen_restores_wide_literal_domains_from_linked_signatures() {
    let temp = write_fixture();
    let output = run_fixture(&temp);
    assert_eq!(
        output.semantic.reachable_helpers.len(),
        4,
        "all four helper definitions must be in the exact final closure"
    );

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final wide-literal executable");
    let node = available_node();
    let node_dump = temp.path().join("node-browser-acceptance");
    let mut runtime_command = Command::new(executable);
    if node.is_some() {
        runtime_command.env("RSJS_NODE_DUMP_DIR", &node_dump);
    }
    let runtime = runtime_command.output().unwrap();
    assert!(
        runtime.status.success(),
        "final wide-literal executable failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&runtime.stdout),
        String::from_utf8_lossy(&runtime.stderr)
    );
    assert!(String::from_utf8(runtime.stdout)
        .unwrap()
        .contains("data-wide-literal-probe"));

    if let Some(node) = node {
        assert_node_evaluates_exact_final_fragment(&node, &node_dump);
    }
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
    fs::write(temp.path().join("src/main.rs"), APP_SOURCE).unwrap();
    temp
}

fn run_fixture(temp: &TempDir) -> proa_cli::rsjs::pipeline::LinkedCargoOutput {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let environment = BTreeMap::new();
    let package_id = fixture_package_id(temp.path(), &cargo, &environment);

    run_linked_cargo(&LinkedCargoRequest {
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
            target_name: APP_PACKAGE.to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-linked-helper-wide-literal-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: environment,
    })
    .expect("the wide-literal fixture must link and compile")
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
        .to_string()
}

fn available_node() -> Option<OsString> {
    let node = std::env::var_os("NODE").unwrap_or_else(|| OsString::from("node"));
    Command::new(&node)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| node)
}

fn assert_node_evaluates_exact_final_fragment(node: &OsString, directory: &Path) {
    fs::write(
        directory.join("rsjs-runtime.js"),
        r#"
export function walk_path() { return {}; }
function makeSignal(initial) {
    let value = initial;
    return {
        get() { return value; },
        set(next) { value = next; },
        update(update) { value = update(value); },
        subscribe() { return () => {}; },
    };
}
export const signal = makeSignal;
export function integer_signal(initial) { return makeSignal(initial); }
export function read_signal_initials() { return []; }
export function signal_initial(initials, index, fallback) {
    return initials[index] ?? fallback;
}
export function signal_initial_integer(initials, index, fallback) {
    return initials[index] ?? fallback;
}
export function bind_text(_node, read) {
    globalThis.__wideLiteralValues.push(read());
    return () => {};
}
export function bind_conditional() { return () => {}; }
export function dispose_all(disposers) {
    while (disposers.length > 0) {
        const dispose = disposers.pop();
        if (typeof dispose === "function") dispose();
    }
}
"#,
    )
    .unwrap();
    fs::write(
        directory.join("acceptance.mjs"),
        r#"
import { readFileSync } from "node:fs";

globalThis.__wideLiteralValues = [];
const fragment = await import("./component.js");
const exportName = readFileSync(new URL("./export-name.txt", import.meta.url), "utf8");
const mount = fragment[exportName];
if (typeof mount !== "function") {
    throw new Error(`missing exact fragment export ${JSON.stringify(exportName)}`);
}
const dispose = mount({}, { signalSeeds: { 0: 0 } });
const values = globalThis.__wideLiteralValues;
const expected = [
    9007199254740993n,
    -9007199254740991n,
    9007199254740995n,
    1,
    9007199254740999n,
];
if (values.length !== expected.length || values.some((value, index) => value !== expected[index])) {
    throw new Error(`wide helper fragment produced ${String(values)}, expected ${String(expected)}`);
}
if (typeof values[0] !== "bigint" || typeof values[1] !== "bigint" || typeof values[2] !== "bigint" || typeof values[4] !== "bigint") {
    throw new Error(`wide helper results lost BigInt representation: ${values.map((value) => typeof value)}`);
}
if (typeof dispose !== "function") {
    throw new Error("wide helper fragment did not return a disposer");
}
dispose();
"#,
    )
    .unwrap();

    let output = Command::new(node)
        .current_dir(directory)
        .arg("acceptance.mjs")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Node rejected exact final wide-literal fragment\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
