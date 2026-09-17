// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for an exact `NodeRef<T>` handle passed
//! through a participant-only bridge and renamed direct component edges.
//!
//! This fixture exercises descriptor compilation, rustc-resolved alias
//! linking, schema-15 semantic planning, final fragment/owner generation, and
//! execution of the resulting linked binary. Neither participant can hydrate
//! standalone: the bridge must forward, and the leaf must bind, the exact
//! object created and used by the root owner's focus handler.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use proa_cli::rsjs::abi_adapter::ComponentNodeRefBindTarget;
use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, DescriptorRootSelector, LinkedCargoOutput, LinkedCargoRequest,
};
use proa_cli::rsjs::semantic_link::{
    StandaloneContextRequirement, StandaloneRootDisposition, SEMANTIC_LINK_SCHEMA_VERSION,
};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, DescriptorBody, InvocationId, ProducerVersion,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

static FIXTURE_BUILD_LOCK: Mutex<()> = Mutex::new(());

const NODE_REF_ALIAS_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};
use rsjs::rsjs;
use std::fs;

mod input_component {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct FocusInput {
        pub target: rsjs::NodeRef<rsjs::HtmlInputElement>,
    }

    // This component participates in its enclosing owner but does not
    // originate client behavior. Its required NodeRef is incoming context.
    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for FocusInput {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <input
                    data-node-ref-participant
                    aria-label="Exact focus target"
                    ref={self.target}
                />
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::FocusInput as RenamedFocusInput;
    }
}

mod bridge_component {
    use super::*;
    use crate::input_component::public::RenamedFocusInput as ImportedFocusInput;
    use proa_core::WebRenderSync as RenderSync;

    pub struct FocusBridge {
        pub target: rsjs::NodeRef<rsjs::HtmlInputElement>,
    }

    // The only use of `self.target` is another direct component assignment.
    // The macro cannot guess its role from syntax; rustc-linked child metadata
    // must resolve this deferred projection as the same NodeRef identity.
    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for FocusBridge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <section data-node-ref-bridge>
                    {ImportedFocusInput {
                        target: self.target,
                    }}
                </section>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::FocusBridge as RenamedFocusBridge;
    }
}

mod focus_page {
    use super::*;
    use crate::bridge_component::public::RenamedFocusBridge as ImportedFocusBridge;
    use proa_core::WebRenderSync as RenderSync;

    pub struct FocusPage;

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for FocusPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let focus_target = rsjs::node_ref::<rsjs::HtmlInputElement>();
            let focus = rsjs::event_handler(
                |_: rsjs::MouseEvent<rsjs::elem::Button>| focus_target.focus(),
            );

            proa_macros::html_sync! {
                <main data-node-ref-page>
                    {ImportedFocusBridge {
                        target: focus_target,
                    }}
                    <button
                        type="button"
                        data-focus-exact-input
                        on_click={focus()}
                    >
                        "Focus exact input"
                    </button>
                </main>
            }
            .render(cx)
        }
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let html = proa_core::render_to_string(focus_page::FocusPage);
    assert_eq!(
        html.matches("data-rsjs-island-root=\"rsjs_owner_").count(),
        1,
        "the root and both participants must form exactly one logical owner: {html}"
    );
    assert_eq!(
        html.matches("data-rsjs-island-root=\"rsjs_member_").count(),
        2,
        "the fused bridge and leaf must retain two member markers inside that owner: {html}"
    );
    assert_eq!(
        html.matches("data-node-ref-participant").count(),
        1,
        "the participant must render exactly one input: {html}"
    );
    assert!(
        html.contains("data-node-ref-page")
            && html.contains("data-node-ref-bridge")
            && html.contains("data-focus-exact-input")
            && html.contains("data-rsjs-handler=")
            && html.contains("data-rsjs-ref="),
        "SSR must retain both the focus handler and child ref marker: {html}"
    );

    let manifest = rsjs::manifest::manifest();
    let parent_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "FocusPage")
        .expect("FocusPage fragment");
    let bridge_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "FocusBridge")
        .expect("FocusBridge fragment");
    let child_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "FocusInput")
        .expect("FocusInput fragment");

    assert_eq!(
        parent_fragment.js_source.matches(" = { current: null };").count(),
        1,
        "the parent fragment must allocate exactly one ref object:\n{}",
        parent_fragment.js_source
    );
    assert!(
        parent_fragment
            .js_source
            .contains("options.expose.nodeRefs[0] = __ref_0;")
            && parent_fragment
                .js_source
                .contains("__ref_0.current?.focus?.();")
            && parent_fragment
                .js_source
                .contains("__rsjs_disposers.push(() => { __ref_0.current = null; });"),
        "the parent handler and export must use the same ref object:\n{}",
        parent_fragment.js_source
    );
    assert!(
        bridge_fragment
            .js_source
            .contains("const __ref_0 = options.nodeRefs?.[0];")
            && bridge_fragment
                .js_source
                .contains("linked NodeRef alias 0 was not supplied")
            && bridge_fragment
                .js_source
                .contains("options.expose.nodeRefs[0] = __ref_0;")
            && !bridge_fragment
                .js_source
                .contains("__ref_0.current = __node_")
            && !bridge_fragment
                .js_source
                .contains(" = { current: null };"),
        "the participant-only bridge must import and re-export the exact object without binding or allocating it:\n{}",
        bridge_fragment.js_source
    );
    assert!(
        child_fragment
            .js_source
            .contains("const __ref_0 = options.nodeRefs?.[0];")
            && child_fragment
                .js_source
                .contains("linked NodeRef alias 0 was not supplied")
            && child_fragment.js_source.contains("__ref_0.current = __node_0;")
            && child_fragment
                .js_source
                .contains("__rsjs_disposers.push(() => { __ref_0.current = null; });")
            && !child_fragment
                .js_source
                .contains(" = { current: null };"),
        "the participant fragment must import and bind the exact object without fallback allocation:\n{}",
        child_fragment.js_source
    );

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
    assert_eq!(
        rendered_owners.len(),
        1,
        "the final executable must activate one linked owner"
    );
    let owner = rendered_owners[0];
    assert!(
        owner
            .component_fragment_imports
            .contains(&parent_fragment.asset_id)
            && owner
                .component_fragment_imports
                .contains(&bridge_fragment.asset_id)
            && owner
                .component_fragment_imports
                .contains(&child_fragment.asset_id),
        "the owner must import all three linked fragments"
    );
    assert!(
        owner
            .js_source
            .contains("const __rsjs_node_ref_1_0 = __rsjs_expose_0?.nodeRefs?.[0];")
            && owner
                .js_source
                .contains("__rsjs_node_refs_1[0] = __rsjs_node_ref_1_0;")
            && owner.js_source.contains("nodeRefs: __rsjs_node_refs_1,")
            && owner
                .js_source
                .contains("const __rsjs_node_ref_2_0 = __rsjs_expose_1?.nodeRefs?.[0];")
            && owner
                .js_source
                .contains("__rsjs_node_refs_2[0] = __rsjs_node_ref_2_0;")
            && owner.js_source.contains("nodeRefs: __rsjs_node_refs_2,")
            && !owner
                .js_source
                .contains(" = { current: null }"),
        "the owner must forward the root object through the bridge into the leaf without allocating a fallback:\n{}",
        owner.js_source
    );
    let parent_mount = owner
        .js_source
        .find("const __rsjs_fragment_disposer_0")
        .expect("parent fragment mount");
    let bridge_mount = owner
        .js_source
        .find("const __rsjs_fragment_disposer_1")
        .expect("bridge fragment mount");
    let child_mount = owner
        .js_source
        .find("const __rsjs_fragment_disposer_2")
        .expect("leaf fragment mount");
    assert!(
        parent_mount < bridge_mount
            && bridge_mount < child_mount
            && owner
                .js_source
                .contains("return () => dispose_all(__rsjs_disposers, \"linked owner teardown\");"),
        "the owner must push parent cleanup before child cleanup so LIFO teardown clears the child binding first:\n{}",
        owner.js_source
    );

    if let Some(directory) = std::env::var_os("RSJS_NODE_DUMP_DIR") {
        let directory = std::path::PathBuf::from(directory);
        fs::create_dir_all(&directory).expect("create Node acceptance directory");
        fs::write(
            directory.join("package.json"),
            "{\"type\":\"module\"}",
        )
        .expect("write Node package marker");
        for fragment in [parent_fragment, bridge_fragment, child_fragment] {
            let source = fragment
                .js_source
                .replace("from \"/rsjs-runtime.js\"", "from \"./rsjs-runtime.js\"");
            fs::write(
                directory.join(format!("component_{}.js", fragment.asset_id)),
                source,
            )
            .expect("write linked component fragment");
        }
        let owner_source = owner
            .js_source
            .replace("from \"/rsjs-runtime.js\"", "from \"./rsjs-runtime.js\"");
        fs::write(directory.join("owner.js"), owner_source)
            .expect("write linked owner module");
    }

    print!("{html}");
}
"#;

#[test]
fn renamed_participant_chain_receives_one_exact_root_node_ref() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = write_fixture("rsjs_node_ref_handle_alias_fixture", NODE_REF_ALIAS_FIXTURE);
    let output = run_fixture(
        &temp,
        "rsjs_node_ref_handle_alias_fixture",
        "rsjs-node-ref-handle-alias",
    );

    assert_exact_node_ref_plan(&output);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final NodeRef alias executable");
    let node = available_node();
    let node_dump = temp.path().join("node-browser-acceptance");
    let mut runtime_command = Command::new(executable);
    if node.is_some() {
        runtime_command.env("RSJS_NODE_DUMP_DIR", &node_dump);
    }
    let runtime = runtime_command.output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(
        html.contains("data-node-ref-page")
            && html.contains("data-node-ref-bridge")
            && html.contains("data-node-ref-participant")
            && html.contains("data-focus-exact-input"),
        "the cargo-proa linked executable must render the complete owner: {html}"
    );
    if let Some(node) = node {
        assert_node_focuses_exact_child_input(&node, &node_dump);
    }
}

fn assert_exact_node_ref_plan(output: &LinkedCargoOutput) {
    assert_eq!(
        SEMANTIC_LINK_SCHEMA_VERSION, 15,
        "this acceptance fixture pins the current authenticated semantic-plan schema"
    );
    let plan_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&output.semantic_plan_path).unwrap()).unwrap();
    assert_eq!(plan_json["schema_version"], SEMANTIC_LINK_SCHEMA_VERSION);
    assert_eq!(
        plan_json["node_ref_aliases"]
            .as_array()
            .expect("schema-15 NodeRef alias array")
            .len(),
        2,
        "the serialized final-plan input must contain both exact aliases"
    );

    let root_key = component_key(
        "rsjs_node_ref_handle_alias_fixture",
        "FocusPage",
        ["focus_page"],
    );
    let bridge_key = component_key(
        "rsjs_node_ref_handle_alias_fixture",
        "FocusBridge",
        ["bridge_component"],
    );
    let leaf_key = component_key(
        "rsjs_node_ref_handle_alias_fixture",
        "FocusInput",
        ["input_component"],
    );
    let plan = &output.semantic;
    assert_eq!(
        plan.node_ref_aliases.len(),
        2,
        "the semantic plan must preserve both hops of the exact NodeRef chain"
    );
    let site_for = |candidate| {
        plan.sites
            .iter()
            .find(|site| site.candidate == candidate)
            .expect("NodeRef alias site")
    };
    let root_to_bridge = plan
        .node_ref_aliases
        .iter()
        .find(|alias| site_for(alias.child).component == bridge_key)
        .expect("root-to-bridge NodeRef alias");
    let bridge_to_leaf = plan
        .node_ref_aliases
        .iter()
        .find(|alias| site_for(alias.child).component == leaf_key)
        .expect("bridge-to-leaf NodeRef alias");

    assert_eq!(site_for(root_to_bridge.parent).component, root_key);
    assert_eq!(site_for(root_to_bridge.child).component, bridge_key);
    assert_eq!(root_to_bridge.bind_target, None);
    assert_eq!(
        bridge_to_leaf.parent, root_to_bridge.child,
        "the bridge occurrence must be the exact producer for the leaf hop"
    );
    assert_eq!(site_for(bridge_to_leaf.parent).component, bridge_key);
    assert_eq!(
        site_for(bridge_to_leaf.child).component,
        leaf_key,
        "the renamed leaf import must resolve to its defining component identity"
    );
    assert_eq!(
        bridge_to_leaf.bind_target,
        Some(ComponentNodeRefBindTarget::Input)
    );
    for alias in [root_to_bridge, bridge_to_leaf] {
        let child_site = site_for(alias.child);
        assert_eq!(child_site.parent, Some(alias.parent));
        assert_eq!(child_site.incoming_edge, Some(alias.edge));
    }

    let bridge_component = plan
        .components
        .iter()
        .find(|component| component.semantic_key == bridge_key)
        .expect("canonical bridge component");
    let DescriptorBody::Component(bridge_body) = &bridge_component.descriptor.body else {
        panic!("FocusBridge descriptor must be a component");
    };
    assert_eq!(bridge_body.fields.len(), 1);
    assert_eq!(bridge_component.codegen.inbound_node_refs.len(), 1);
    assert_eq!(
        bridge_component.codegen.inbound_node_refs[0].bind_target,
        None
    );
    assert_eq!(
        root_to_bridge.child_node_ref,
        bridge_component.codegen.inbound_node_refs[0].producer_node_ref_id
    );

    let leaf_component = plan
        .components
        .iter()
        .find(|component| component.semantic_key == leaf_key)
        .expect("canonical leaf component");
    let DescriptorBody::Component(leaf_body) = &leaf_component.descriptor.body else {
        panic!("FocusInput descriptor must be a component");
    };
    assert_eq!(leaf_body.fields.len(), 1);
    assert_eq!(leaf_component.codegen.inbound_node_refs.len(), 1);
    assert_eq!(
        leaf_component.codegen.inbound_node_refs[0].bind_target,
        Some(ComponentNodeRefBindTarget::Input)
    );
    assert_eq!(
        bridge_to_leaf.child_node_ref,
        leaf_component.codegen.inbound_node_refs[0].producer_node_ref_id
    );

    for (component, alias, bind_target) in [
        (bridge_key.clone(), root_to_bridge, None),
        (
            leaf_key.clone(),
            bridge_to_leaf,
            Some(ComponentNodeRefBindTarget::Input),
        ),
    ] {
        let root = plan
            .root_decisions
            .iter()
            .find(|decision| decision.component == component)
            .expect("standalone-root decision for participant");
        assert!(
            matches!(
                &root.disposition,
                StandaloneRootDisposition::ContextRequired { missing }
                    if matches!(
                        missing.as_slice(),
                        [StandaloneContextRequirement::NodeRef(requirement)]
                            if requirement.consumer == component
                                && requirement.structural_path.is_empty()
                                && requirement.field == alias.field
                                && requirement.crossing_hook == alias.crossing_hook
                                && requirement.node_ref == alias.child_node_ref
                                && requirement.bind_target == bind_target
                    )
            ),
            "each participant must require its exact incoming NodeRef context"
        );
    }

    assert_eq!(
        plan.owners.owners.len(),
        1,
        "the linked chain must produce one browser owner"
    );
    let owner = &plan.owners.owners[0];
    assert_eq!(
        owner.candidates.len(),
        3,
        "the owner must contain the root, bridge, and leaf"
    );
    assert_eq!(
        plan.owners.candidate_to_owner.get(&root_to_bridge.parent),
        Some(&owner.id)
    );
    for alias in [root_to_bridge, bridge_to_leaf] {
        assert_eq!(
            plan.owners.candidate_to_owner.get(&alias.child),
            Some(&owner.id),
            "a NodeRef handle may not cross an owner boundary"
        );
    }
}

fn component_key<I>(package: &str, name: &str, module_path: I) -> ComponentSemanticKey
where
    I: IntoIterator,
    I::Item: Into<String>,
{
    ComponentSemanticKey::v1(package, package, module_path, name).unwrap()
}

fn write_fixture(package: &str, source: &str) -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = {package:?}\n\
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
    fs::write(temp.path().join("src/main.rs"), source).unwrap();
    temp
}

fn run_fixture(temp: &TempDir, package: &str, invocation: &str) -> LinkedCargoOutput {
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
            target_name: package.to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive(invocation),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: environment,
    })
    .expect("the exact NodeRef alias fixture must link and compile")
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
    metadata["packages"][0]["id"].as_str().unwrap().to_string()
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

fn assert_node_focuses_exact_child_input(node: &OsString, directory: &Path) {
    fs::write(
        directory.join("rsjs-runtime.js"),
        r#"
export function signal(initial) {
    let value = initial;
    return {
        get() { return value; },
        set(next) { value = next; },
        subscribe() { return () => {}; },
    };
}

export function bind_text() { return () => {}; }
export function bind_conditional() { return () => {}; }
export function walk_path(root) { return root; }
export function dispose_all(disposers) {
    while (disposers.length > 0) {
        const dispose = disposers.pop();
        if (typeof dispose === "function") dispose();
    }
}
export function bind_delegated_events(_root, event, handlers) {
    if (event !== "click") throw new Error(`unexpected delegated event ${event}`);
    globalThis.__nodeRefAcceptanceHandlers = handlers;
    const listener = (browserEvent) => {
        const marker = browserEvent.target?.getAttribute?.("data-rsjs-handler");
        const handler = handlers.get(marker);
        if (handler != null) handler(browserEvent);
    };
    _root.addEventListener(event, listener);
    return () => _root.removeEventListener(event, listener);
}
export function hydrate(_assetId, initialize) {
    globalThis.__nodeRefAcceptanceDispose =
        initialize(globalThis.__nodeRefAcceptanceOwnerRoot);
}
export function hydrate_from_module(_moduleUrl, _strategy) {
    return (_assetId, initialize) => {
        globalThis.__nodeRefAcceptanceDispose =
            initialize(globalThis.__nodeRefAcceptanceOwnerRoot);
    };
}
export function report_hydration_error(scope, error) {
    throw new Error(`${scope}: ${error?.stack ?? error}`);
}
"#,
    )
    .unwrap();
    fs::write(
        directory.join("acceptance.mjs"),
        r#"
import { readFileSync } from "node:fs";

const ownerSource = readFileSync(new URL("./owner.js", import.meta.url), "utf8");
const rootCandidate = ownerSource.match(
    /getAttribute\("data-rsjs-linked-candidate"\) !== "([^"]+)"/,
)?.[1];
const seedKeys = [...ownerSource.matchAll(
    /getAttribute\("data-rsjs-island-root"\) !== "([^"]+)"/g,
)].map((match) => match[1]);
const childCandidates = [...ownerSource.matchAll(
    /data-rsjs-linked-candidate=\\"([^"]+)\\"/g,
)].map((match) => match[1]);
if (
    rootCandidate == null
    || seedKeys.length !== 3
    || childCandidates.length !== 2
) {
    throw new Error(`could not recover exact owner wrapper protocol:\n${ownerSource}`);
}

const input = {
    focusCalls: 0,
    focus() {
        this.focusCalls += 1;
    },
};
const listeners = new Map();
const parentElement = {
    addEventListener(event, listener) {
        listeners.set(event, listener);
    },
    removeEventListener(event, listener) {
        if (listeners.get(event) === listener) listeners.delete(event);
    },
    dispatchEvent(event, browserEvent) {
        listeners.get(event)?.(browserEvent);
    },
};
const bridgeElement = {};
const bridgeWrapper = {
    children: [bridgeElement],
    firstElementChild: bridgeElement,
    getAttribute(name) {
        if (name === "data-rsjs-island-root") return seedKeys[1];
        if (name === "data-rsjs-linked-candidate") return childCandidates[0];
        return null;
    },
    contains(node) {
        return node === childWrapper;
    },
};
const childWrapper = {
    children: [input],
    firstElementChild: input,
    getAttribute(name) {
        if (name === "data-rsjs-island-root") return seedKeys[2];
        if (name === "data-rsjs-linked-candidate") return childCandidates[1];
        return null;
    },
    contains() {
        return false;
    },
};
const ownerRoot = {
    children: [parentElement],
    firstElementChild: parentElement,
    getAttribute(name) {
        if (name === "data-rsjs-linked-candidate") return rootCandidate;
        if (name === "data-rsjs-island-root") return seedKeys[0];
        return null;
    },
    querySelectorAll(selector) {
        const candidate = selector.match(
            /^\[data-rsjs-linked-candidate="([^"]+)"\]$/,
        )?.[1];
        return [bridgeWrapper, childWrapper].filter(
            (wrapper) =>
                wrapper.getAttribute("data-rsjs-linked-candidate") === candidate,
        );
    },
    contains(node) {
        return node === bridgeWrapper || node === childWrapper;
    },
};
globalThis.__nodeRefAcceptanceOwnerRoot = ownerRoot;

await import("./owner.js");

const handlers = globalThis.__nodeRefAcceptanceHandlers;
if (!(handlers instanceof Map) || handlers.size !== 1) {
    throw new Error(`expected one delegated focus handler, received ${handlers}`);
}
const handler = [...handlers.values()][0];
const marker = [...handlers.keys()][0];
const clickEvent = {
    currentTarget: {},
    target: {
        getAttribute(name) {
            if (name === "data-rsjs-handler") return marker;
            return null;
        },
    },
    preventDefault() {},
    stopPropagation() {},
};
parentElement.dispatchEvent("click", clickEvent);
if (input.focusCalls !== 1) {
    throw new Error(
        `delegated parent focus handler did not reach the leaf input through the exact shared NodeRef object; focusCalls=${input.focusCalls}`,
    );
}

const dispose = globalThis.__nodeRefAcceptanceDispose;
if (typeof dispose !== "function") {
    throw new Error("linked owner did not return its teardown function");
}
dispose();
if (listeners.has("click")) {
    throw new Error("linked owner teardown did not remove the delegated click listener");
}
handler({
    currentTarget: {},
    target: {},
    preventDefault() {},
    stopPropagation() {},
});
if (input.focusCalls !== 1) {
    throw new Error(
        `disposed NodeRef still reached the child input; focusCalls=${input.focusCalls}`,
    );
}
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
        "Node browser acceptance failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
