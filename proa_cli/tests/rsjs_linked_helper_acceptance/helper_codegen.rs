// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for standalone `#[rsjs::client]` helper
//! linking.
//!
//! This fixture deliberately crosses every phase boundary: descriptor
//! compilation records symbolic helper calls, rustc relocations resolve normal
//! imports and re-exports, the semantic linker computes the transitive helper
//! closure, final proc-macro expansion emits deployable component JavaScript,
//! and (when Node is available) that exact fragment is executed. Plan-shape
//! assertions alone cannot prove that helper bodies were emitted or that final
//! call sites stopped referring to descriptor-only sentinels.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, DescriptorRootSelector, LinkedCargoOutput, LinkedCargoRequest,
};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, HelperSemanticKey, InvocationId, ProducerVersion,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

const APP_PACKAGE: &str = "rsjs_linked_helper_codegen_fixture";
const DEP_PACKAGE: &str = "rsjs_linked_helper_codegen_dep";
const DEP_CRATE: &str = "helper_dep";

const DEP_SOURCE: &str = r#"
mod implementation {
    #[rsjs::client]
    pub fn leaf(value: i32) -> i32 {
        value * 17_i32
    }

    #[rsjs::client]
    pub fn serial(value: i32) -> i32 {
        leaf(value) - 23_i32
    }
}

pub mod public {
    pub use super::implementation::serial as reexported_serial;
}
"#;

const APP_SOURCE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_assignments, unused_imports)]

use std::fs;

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{rsjs, signal};

// This module deliberately shadows the Cargo dependency alias. In Rust 2021,
// the leading `::` below selects the external-prelude crate; dropping it while
// reconstructing the helper-to-helper relocation instead selects this helper.
mod external_helper_dep {
    pub mod public {
        #[rsjs::client]
        pub fn reexported_serial(value: i32) -> i32 {
            value * 101_i32 + 7001_i32
        }
    }
}

#[rsjs::client]
fn local_serial(value: i32) -> i32 {
    ::external_helper_dep::public::reexported_serial(value) + 31_i32
}

#[rsjs::client]
pub mod object_logic {
    #[derive(Clone, Copy)]
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn new(seed: i32) -> Self {
            Self { count: seed }
        }

        pub fn adjust(&mut self, amount: i32) {
            if amount > 0_i32 {
                let next: i32 = self.count + amount;
                self.count = next;
            } else {
                let next: i32 = self.count - amount;
                self.count = next;
            }
        }

        pub fn value(&self) -> i32 {
            self.count
        }
    }

    pub fn adjusted(seed: i32) -> i32 {
        let original = Counter::new(seed);
        let mut counter = original;
        let mut amount = 0;
        amount = 11;
        counter.adjust(amount);
        original.value() + counter.value()
    }
}

struct LatentHelperChild {
    value: f64,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LatentHelperChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(self.value);
        proa_macros::html_sync! {
            <span data-latent-helper-child>{value.get()}</span>
        }
        .render(cx)
    }
}

struct CalendarProbe;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for CalendarProbe {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let view = signal(1_i32);
        // This helper call has no local DOM consumer. It becomes browser-live
        // only because the linked child receives the memo cell as a live
        // prop, exercising the final fragment's forced-root closure.
        let latent = rsjs::memo(|| if local_serial(view.get() + 20_i32) > 0_i32 {
                1.0_f64
            } else {
                0.0_f64
            });
        proa_macros::html_sync! {
            <main data-calendar-helper-probe>
                <output>{local_serial(view.get() + 0_i32)}</output>
                <output>{local_serial(view.get() + 1_i32)}</output>
                <output>{local_serial(view.get() + 2_i32)}</output>
                <output>{local_serial(view.get() + 3_i32)}</output>
                <output>{local_serial(view.get() + 4_i32)}</output>
                <output>{local_serial(view.get() + 5_i32)}</output>
                <output>{local_serial(view.get() + 6_i32)}</output>
                <output>{local_serial(view.get() + 7_i32)}</output>
                <output>{local_serial(view.get() + 8_i32)}</output>
                <output>{local_serial(view.get() + 9_i32)}</output>
                <output>{local_serial(view.get() + 10_i32)}</output>
                <output>{local_serial(view.get() + 11_i32)}</output>
                <output>{local_serial(view.get() + 12_i32)}</output>
                <output>{local_serial(view.get() + 13_i32)}</output>
                <output>{object_logic::adjusted(view.get())}</output>
                {LatentHelperChild { value: latent.get() }}
            </main>
        }
        .render(cx)
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let html = proa_core::render_to_string(CalendarProbe);
    assert_eq!(html.matches("<output").count(), 15, "{html}");
    assert_eq!(
        html.matches("data-rsjs-bind=").count(),
        16,
        "the fifteen Calendar bindings plus the linked child prop binding must survive: {html}"
    );
    assert!(
        html.contains(">13</output>"),
        "the SSR evaluator must preserve scalar reassignment and Copy value semantics: {html}"
    );

    let manifest = rsjs::manifest::manifest();
    assert!(
        manifest.client_modules.is_empty(),
        "standalone helper linking must not depend on legacy client-module registration"
    );
    for island in &manifest.islands {
        assert!(
            !island.js_source.contains("__rsjs_unlinked_"),
            "deployable owner {} retained an unresolved helper sentinel:\n{}",
            island.asset_id,
            island.js_source
        );
    }
    for fragment in &manifest.component_fragments {
        assert!(
            !fragment.js_source.contains("__rsjs_unlinked_"),
            "deployable fragment {} retained an unresolved helper sentinel:\n{}",
            fragment.asset_id,
            fragment.js_source
        );
    }

    let fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "CalendarProbe")
        .expect("CalendarProbe final component fragment");
    let js = fragment.js_source;
    assert_eq!(
        js.matches("function __rsjs_helper_").count(),
        8,
        "the scalar chain plus object root, clone, constructor, mutator, and reader must each be emitted once:\n{js}"
    );

    // Dense helper IDs derive from canonical helper identity, not source
    // spelling. Whichever identity sorts first, the root helper appears once
    // as a declaration plus fifteen component calls; the middle and leaf
    // helpers each appear once as a declaration plus one transitive call.
    let mut helper_reference_counts = (0_u32..8)
        .map(|id| js.matches(&format!("__rsjs_helper_{id}(")).count())
        .collect::<Vec<_>>();
    helper_reference_counts.sort_unstable();
    assert_eq!(
        helper_reference_counts,
        vec![2, 2, 2, 2, 2, 2, 3, 16],
        "the repeated scalar root and seven transitive scalar/object definitions must each remain linked exactly once:\n{js}"
    );

    // These deliberately unique constants are executable body markers. A
    // count above one would mean the implementation copied a helper body per
    // call site instead of emitting the reachable helper definition once.
    for marker in ["17, \"i32\"", "23, \"i32\"", "31, \"i32\""] {
        assert_eq!(
            js.matches(marker).count(),
            1,
            "helper body marker `{marker}` must occur exactly once:\n{js}"
        );
    }

    let rendered_owners = manifest
        .islands
        .iter()
        .filter(|island| {
            html.contains(&format!(
                "data-rsjs-island-root=\"{}\"",
                island.asset_id
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered_owners.len(), 1, "{html}");
    assert!(
        rendered_owners[0]
            .component_fragment_imports
            .contains(&fragment.asset_id),
        "the rendered owner must import the helper-bearing CalendarProbe fragment"
    );

    if let Some(directory) = std::env::var_os("RSJS_NODE_DUMP_DIR") {
        let directory = std::path::PathBuf::from(directory);
        fs::create_dir_all(&directory).expect("create helper Node acceptance directory");
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
        .expect("write exact final helper-bearing fragment");
    }

    print!("{html}");
}
"#;

#[test]
fn final_codegen_links_repeated_helpers_and_preserves_absolute_external_paths() {
    let temp = write_fixture();
    let output = run_fixture(&temp);

    assert_exact_helper_graph(&output);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final helper-link executable");
    let node = available_node();
    let node_dump = temp.path().join("node-browser-acceptance");
    let mut runtime_command = Command::new(executable);
    if node.is_some() {
        runtime_command.env("RSJS_NODE_DUMP_DIR", &node_dump);
    }
    let runtime = runtime_command.output().unwrap();
    assert!(
        runtime.status.success(),
        "final linked-helper executable failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&runtime.stdout),
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(html.contains("data-calendar-helper-probe"), "{html}");

    if let Some(node) = node {
        assert_node_evaluates_exact_final_fragment(&node, &node_dump);
    }
}

fn assert_exact_helper_graph(output: &LinkedCargoOutput) {
    let component_key = ComponentSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        std::iter::empty::<&str>(),
        "CalendarProbe",
    )
    .unwrap();
    let local_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        std::iter::empty::<&str>(),
        "local_serial",
    )
    .unwrap();
    let serial_key =
        HelperSemanticKey::v1(DEP_PACKAGE, DEP_CRATE, ["implementation"], "serial").unwrap();
    let leaf_key =
        HelperSemanticKey::v1(DEP_PACKAGE, DEP_CRATE, ["implementation"], "leaf").unwrap();
    let shadow_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        ["external_helper_dep", "public"],
        "reexported_serial",
    )
    .unwrap();
    let object_root_key =
        HelperSemanticKey::v1(APP_PACKAGE, APP_PACKAGE, ["object_logic"], "adjusted").unwrap();
    let object_constructor_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        ["object_logic"],
        "__rsjs_object_counter_method_new",
    )
    .unwrap();
    let object_clone_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        ["object_logic"],
        "__rsjs_object_counter_clone_value",
    )
    .unwrap();
    let object_mutator_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        ["object_logic"],
        "__rsjs_object_counter_method_adjust",
    )
    .unwrap();
    let object_reader_key = HelperSemanticKey::v1(
        APP_PACKAGE,
        APP_PACKAGE,
        ["object_logic"],
        "__rsjs_object_counter_method_value",
    )
    .unwrap();

    let component = output
        .semantic
        .components
        .iter()
        .find(|component| component.semantic_key == component_key)
        .expect("CalendarProbe semantic component");
    assert_eq!(
        component
            .helper_edges
            .iter()
            .map(|edge| edge.call.index())
            .collect::<Vec<_>>(),
        (0_u32..16).collect::<Vec<_>>(),
        "the scalar and object bindings must remain exact local call edges"
    );
    assert!(
        component
            .helper_edges
            .iter()
            .take(15)
            .all(|edge| edge.target == local_key),
        "the repeated Calendar-shaped scalar occurrences must resolve to one definition"
    );
    assert_eq!(
        component.helper_edges.last().unwrap().target,
        object_root_key,
        "the final binding must select the public object root"
    );
    let expected_closure = BTreeSet::from([
        leaf_key.clone(),
        serial_key.clone(),
        local_key.clone(),
        object_constructor_key.clone(),
        object_clone_key.clone(),
        object_mutator_key.clone(),
        object_reader_key.clone(),
        object_root_key.clone(),
    ]);
    assert_eq!(
        component
            .helper_closure
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected_closure,
        "component helper closure must contain both complete transitive graphs"
    );

    let helper = |key: &HelperSemanticKey| {
        output
            .semantic
            .helpers
            .iter()
            .find(|helper| &helper.semantic_key == key)
            .expect("reachable semantic helper")
    };
    let local = helper(&local_key);
    assert_eq!(local.helper_edges.len(), 1);
    assert_eq!(local.helper_edges[0].target, serial_key);
    assert_ne!(
        local.helper_edges[0].target, shadow_key,
        "the helper relocation must preserve the leading `::` and resolve the Cargo-renamed external re-export, not the same-named local module"
    );
    assert_eq!(
        local.transitive_closure,
        vec![leaf_key.clone(), serial_key.clone(), local_key.clone()]
    );

    let serial = helper(&serial_key);
    assert_eq!(serial.helper_edges.len(), 1);
    assert_eq!(serial.helper_edges[0].target, leaf_key);
    assert_eq!(
        serial.transitive_closure,
        vec![leaf_key.clone(), serial_key.clone()]
    );

    let leaf = helper(&leaf_key);
    assert!(leaf.helper_edges.is_empty());
    assert_eq!(leaf.transitive_closure, vec![leaf_key.clone()]);

    let object_root = helper(&object_root_key);
    assert_eq!(
        object_root
            .helper_edges
            .iter()
            .map(|edge| edge.target.clone())
            .collect::<Vec<_>>(),
        vec![
            object_constructor_key.clone(),
            object_clone_key.clone(),
            object_mutator_key.clone(),
            object_reader_key.clone(),
            object_reader_key.clone(),
        ],
        "the public scalar root must link constructor, value clone, mutator, and reader definitions"
    );
    for key in [
        &object_constructor_key,
        &object_clone_key,
        &object_mutator_key,
        &object_reader_key,
    ] {
        assert!(helper(key).helper_edges.is_empty());
    }
    assert_eq!(
        output
            .semantic
            .reachable_helpers
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected_closure
    );
}

fn write_fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("helper_dep/src")).unwrap();
    let proa_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = {APP_PACKAGE:?}\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [workspace]\n\
             members = [\"helper_dep\"]\n\n\
             [dependencies]\n\
             external_helper_dep = {{ package = {DEP_PACKAGE:?}, path = \"helper_dep\" }}\n\
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
        temp.path().join("helper_dep/Cargo.toml"),
        format!(
            "[package]\n\
             name = {DEP_PACKAGE:?}\n\
             version = \"0.0.0\"\n\
             edition = \"2021\"\n\n\
             [lib]\n\
             name = \"helper_dep\"\n\n\
             [dependencies]\n\
             rsjs = {{ path = {:?} }}\n",
            proa_root.join("rsjs"),
        ),
    )
    .unwrap();
    fs::write(temp.path().join("src/main.rs"), APP_SOURCE).unwrap();
    fs::write(temp.path().join("helper_dep/src/lib.rs"), DEP_SOURCE).unwrap();
    temp
}

fn run_fixture(temp: &TempDir) -> LinkedCargoOutput {
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
        invocation: InvocationId::derive("rsjs-linked-helper-codegen-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: environment,
    })
    .expect("the exact linked-helper fixture must link and compile")
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
export function memo(read) {
    const cell = makeSignal(read());
    cell.get = read;
    cell._dispose = () => {};
    return cell;
}
export function integer_signal(initial) { return makeSignal(initial); }
export function read_signal_initials() { return []; }
export function signal_initial(initials, index, fallback) {
    return initials[index] ?? fallback;
}
export function signal_initial_integer(initials, index, fallback) {
    return initials[index] ?? fallback;
}
export function checked_int_add(left, right) { return left + right; }
export function checked_int_sub(left, right) { return left - right; }
export function checked_int_mul(left, right) { return left * right; }
export function walk_path() { return {}; }
export function bind_text(_node, read) {
    globalThis.__linkedHelperValues.push(read());
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

globalThis.__linkedHelperValues = [];
const fragment = await import("./component.js");
const exportName = readFileSync(new URL("./export-name.txt", import.meta.url), "utf8");
const mount = fragment[exportName];
if (typeof mount !== "function") {
    throw new Error(`missing exact fragment export ${JSON.stringify(exportName)}`);
}
const dispose = mount({}, { signalSeeds: { 0: 1 } });
const actual = [...globalThis.__linkedHelperValues].sort((left, right) => left - right);
const expected = [13, ...Array.from({ length: 14 }, (_, index) => 25 + 17 * index)]
    .sort((left, right) => left - right);
if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
        `linked helper fragment produced ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`,
    );
}
if (typeof dispose !== "function") {
    throw new Error("linked helper fragment did not return a disposer");
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
        "Node linked-helper acceptance failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
