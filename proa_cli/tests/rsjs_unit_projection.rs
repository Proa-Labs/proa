// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Actual Cargo reuse and runtime coherence across local and shared changes.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::final_compile::FinalCargoError;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo_cached, DescriptorRootSelector, LinkedCargoError, LinkedCargoOutput,
    LinkedCargoRequest,
};
use rsjs_ir::abi::{AbiVersion, InvocationId, ProducerVersion};
use rsjs_ir::build::ArtifactRole;
use serde_json::Value;
use tempfile::TempDir;

const CHILD: &str = r#"
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use rsjs::rsjs;

pub struct Child {
    pub value: i32,
}

#[rsjs(component)]
impl<L: DataLoader> WebRenderSync<L> for Child {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <p data-child>{self.value}</p>
        }
        .render(cx)
    }
}
"#;

const OWNER: &str = r#"
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use rsjs::{rsjs, signal};
CHILD_IMPORT

pub struct OWNER_TYPE;

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for OWNER_TYPE {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(1_i32);
        proa_macros::html_sync! {
            <section data-projection-owner="OWNER_TYPE">
                <span>{shared_helper::advance(count.get())}</span>
                CHILD_SLOT
            </section>
        }
        .render(cx)
    }
}
"#;

const APP: &str = r#"
#![allow(unexpected_cfgs)]
#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    use proa_core::WebRenderSync;
    use projection_owner_a::OwnerA;
    use projection_owner_b::OwnerB;
    let mut cx = proa_core::WebContext::new();
    proa_macros::html_sync! {
        <main>
            {OwnerA}
            {OwnerB}
        </main>
    }
    .render(&mut cx)
    .unwrap();
    let html = cx.into_output().into_string();
    let manifest = rsjs::manifest::manifest();
    let owners = manifest.islands.iter().filter(|owner|
        html.contains(&format!("data-rsjs-island-root=\"{}\"", owner.asset_id)))
        .map(|owner| serde_json::json!({
            "asset": owner.asset_id, "js": owner.js_source,
            "imports": owner.component_fragment_imports,
        })).collect::<Vec<_>>();
    let fragments = manifest.component_fragments.iter().map(|fragment| serde_json::json!({
        "name": fragment.struct_name, "asset": fragment.asset_id,
        "js": fragment.js_source, "export": fragment.export_name,
    })).collect::<Vec<_>>();
    println!("{}", serde_json::json!({"html": html, "owners": owners, "fragments": fragments}));
}
"#;

#[test]
fn unrelated_owner_stays_fresh_and_shared_changes_refresh_every_consumer() {
    let temp = TempDir::new().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let _cleanup = if std::env::var_os("PROA_KEEP_PROJECTION_FIXTURE").is_some() {
        temp.keep();
        eprintln!("projection fixture: {}", root.display());
        None
    } else {
        Some(temp)
    };
    write_fixture(&root);
    let request = fixture_request(&root);
    let first = build(&request);
    let first_runtime = runtime(&first, &root, 2, 1);

    // Change only A's authored template. B has an independent fragment and
    // shares only an unchanged helper crate with A.
    let owner_a_path = root.join("projection_owner_a/src/lib.rs");
    let owner_a = fs::read_to_string(&owner_a_path).unwrap();
    fs::write(
        &owner_a_path,
        format!(
            "{}\n#[allow(dead_code)]\nmod unrelated {{\n{}\n}}\n",
            owner_a
                .replace("<section ", "<article ")
                .replace("</section>", "</article>"),
            owner_source("UnrelatedRoot", false)
        ),
    )
    .unwrap();
    let local = build(&request);
    let local_runtime = runtime(&local, &root, 2, 1);
    assert_ne!(first.semantic.plan_id, local.semantic.plan_id);
    assert_fresh(&local, "projection_owner_a", false);
    assert_fresh(&local, "projection_owner_b", true);
    assert_eq!(
        owner(&first_runtime, "OwnerB"),
        owner(&local_runtime, "OwnerB")
    );
    assert_ne!(
        owner(&first_runtime, "OwnerA")["asset"],
        owner(&local_runtime, "OwnerA")["asset"]
    );

    fs::write(root.join("shared_helper/src/lib.rs"), helper(9)).unwrap();
    let helper_change = build(&request);
    let helper_runtime = runtime(&helper_change, &root, 10, 1);
    for (package, name) in [
        ("projection_owner_a", "OwnerA"),
        ("projection_owner_b", "OwnerB"),
    ] {
        assert_fresh(&helper_change, package, false);
        assert_ne!(
            owner(&local_runtime, name)["asset"],
            owner(&helper_runtime, name)["asset"]
        );
    }

    // B now consumes the same child definition as A. The child unit must
    // contain both occurrences, rather than picking one enclosing owner.
    fs::write(
        root.join("projection_owner_b/src/lib.rs"),
        owner_source("OwnerB", true),
    )
    .unwrap();
    let b_manifest = root.join("projection_owner_b/Cargo.toml");
    let manifest = fs::read_to_string(&b_manifest).unwrap();
    fs::write(
        &b_manifest,
        format!("{manifest}shared_child = {{ path = \"../shared_child\" }}\n"),
    )
    .unwrap();
    let shared = build(&request);
    let shared_runtime = runtime(&shared, &root, 10, 1);
    fs::write(
        root.join("shared_child/src/lib.rs"),
        CHILD
            .replace("<p data-child>", "<strong data-child>")
            .replace("</p>", "</strong>"),
    )
    .unwrap();
    let child_change = build(&request);
    let child_runtime = runtime(&child_change, &root, 10, 1);
    assert_eq!(
        child_runtime["html"]
            .as_str()
            .unwrap()
            .matches("<strong data-child")
            .count(),
        2
    );
    for (package, name) in [
        ("projection_owner_a", "OwnerA"),
        ("projection_owner_b", "OwnerB"),
    ] {
        assert_fresh(&child_change, package, false);
        assert_ne!(
            owner(&shared_runtime, name)["asset"],
            owner(&child_runtime, name)["asset"]
        );
    }

    let unchanged = build(&request);
    for package in [
        "projection_owner_a",
        "projection_owner_b",
        "shared_child",
        "shared_helper",
    ] {
        assert_fresh(&unchanged, package, true);
    }
    assert_eq!(child_runtime, runtime(&unchanged, &root, 10, 1));
    let directory = request.final_target_root.join("rsjs-final-unit-plans");
    let active = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            fs::read_to_string(path).unwrap().contains(
                fragment(&child_runtime, "OwnerB")["asset"]
                    .as_str()
                    .unwrap(),
            )
        })
        .unwrap();
    let projection_hash = active
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .split('.')
        .next()
        .unwrap();
    let dep_info = unchanged.final_cargo.target_directory.join("debug/deps");
    let b_dep = fs::read_dir(dep_info)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("projection_owner_b-")
                && path.extension().is_some_and(|ext| ext == "d")
                && fs::read_to_string(path).unwrap().contains(projection_hash)
        })
        .unwrap();
    let deps = fs::read_to_string(b_dep).unwrap();
    assert!(deps.contains("PROA_RSJS_FINAL_UNIT_PLAN_"), "{deps}");
    assert!(!deps.contains("# env-dep:RSJS_PLAN_ID="), "{deps}");
    assert!(
        !deps.contains("# env-dep:RSJS_LINK_PLAN_FILE_HASH="),
        "{deps}"
    );

    // Even a fully warm Cargo hit must freshly validate immutable projection
    // bytes before the runner accepts the artifact as current.
    fs::write(&active, b"{}").unwrap();
    let error = run_linked_cargo_cached(&request, &root.join("generated/cache")).unwrap_err();
    assert!(
        matches!(error, LinkedCargoError::FinalCargo(FinalCargoError::UnitProjection(ref reason))
        if reason.contains("conflicting content-addressed unit projection")),
        "{error:?}"
    );
}

fn helper(delta: i32) -> String {
    format!("#[rsjs::client]\npub fn advance(value: i32) -> i32 {{ value + {delta}_i32 }}\n")
}

fn owner_source(name: &str, child: bool) -> String {
    OWNER
        .replace("OWNER_TYPE", name)
        .replace(
            "CHILD_IMPORT",
            if child {
                "use shared_child::Child;"
            } else {
                ""
            },
        )
        .replace(
            "CHILD_SLOT",
            if child {
                "{Child { value: count.get() }}"
            } else {
                "<p>{count.get()}</p>"
            },
        )
}

fn write_fixture(root: &Path) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let common = format!("proa_core = {{ path = {:?} }}\nproa_macros = {{ path = {:?} }}\nrsjs = {{ path = {:?} }}\n", repo.join("proa_core"), repo.join("proa_macros"), repo.join("rsjs"));
    let packages = [
        ("projection_app", "src/main.rs", APP.to_owned(), format!("{common}serde_json = \"1\"\nprojection_owner_a = {{ path = \"projection_owner_a\" }}\nprojection_owner_b = {{ path = \"projection_owner_b\" }}\n")),
        ("projection_owner_a", "src/lib.rs", owner_source("OwnerA", true), format!("{common}shared_helper = {{ path = \"../shared_helper\" }}\nshared_child = {{ path = \"../shared_child\" }}\n")),
        ("projection_owner_b", "src/lib.rs", owner_source("OwnerB", false), format!("{common}shared_helper = {{ path = \"../shared_helper\" }}\n")),
        ("shared_child", "src/lib.rs", CHILD.to_owned(), common.clone()),
        ("shared_helper", "src/lib.rs", helper(1), common),
    ];
    for (name, source_path, source, dependencies) in packages {
        let directory = if name == "projection_app" {
            root.to_path_buf()
        } else {
            root.join(name)
        };
        fs::create_dir_all(directory.join("src")).unwrap();
        let workspace = if name == "projection_app" {
            "[workspace]\nmembers = [\"projection_owner_a\", \"projection_owner_b\", \"shared_child\", \"shared_helper\"]\n"
        } else {
            ""
        };
        fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2021\"\n{workspace}\n[dependencies]\n{dependencies}")).unwrap();
        fs::write(directory.join(source_path), source).unwrap();
    }
}

fn fixture_request(root: &Path) -> LinkedCargoRequest {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(&cargo)
        .current_dir(root)
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(metadata.status.success());
    let metadata: Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package_id = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "projection_app")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let generated = root.join("generated");
    LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: root.to_path_buf(),
            staging_directory: generated.join("staging"),
            target_directory: generated.join("descriptor"),
            source_exclusion_directory: generated.clone(),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: Default::default(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: "projection_app".into(),
            target_kinds: BTreeSet::from(["bin".into()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-unit-projection-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: generated.join("plans"),
        final_target_root: generated.join("final"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: Default::default(),
    }
}

fn build(request: &LinkedCargoRequest) -> LinkedCargoOutput {
    run_linked_cargo_cached(
        request,
        &request.descriptor.working_directory.join("generated/cache"),
    )
    .unwrap()
}

fn assert_fresh(output: &LinkedCargoOutput, package: &str, expected: bool) {
    let artifact = output
        .final_cargo
        .cargo_artifacts
        .iter()
        .find(|artifact| artifact.target.name == package)
        .unwrap();
    assert_eq!(
        artifact.fresh, expected,
        "unexpected final Cargo freshness for {package}"
    );
}

fn fragment<'a>(runtime: &'a Value, name: &str) -> &'a Value {
    runtime["fragments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fragment| fragment["name"] == name)
        .unwrap()
}

fn owner<'a>(runtime: &'a Value, name: &str) -> &'a Value {
    let asset = &fragment(runtime, name)["asset"];
    runtime["owners"]
        .as_array()
        .unwrap()
        .iter()
        .find(|owner| owner["imports"].as_array().unwrap().contains(asset))
        .unwrap()
}

fn runtime(output: &LinkedCargoOutput, root: &Path, helper_value: i32, child_value: i32) -> Value {
    let process = Command::new(output.final_root_artifact.executable.as_ref().unwrap())
        .output()
        .unwrap();
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    let runtime: Value = serde_json::from_slice(&process.stdout).unwrap();
    assert_eq!(runtime["owners"].as_array().unwrap().len(), 2);
    let html = runtime["html"].as_str().unwrap();
    for owner in runtime["owners"].as_array().unwrap() {
        assert!(html.contains(owner["asset"].as_str().unwrap()));
        for imported in owner["imports"].as_array().unwrap() {
            let fragment = runtime["fragments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|fragment| &fragment["asset"] == imported)
                .unwrap();
            assert!(owner["js"]
                .as_str()
                .unwrap()
                .contains(imported.as_str().unwrap()));
            assert!(fragment["js"]
                .as_str()
                .unwrap()
                .contains(fragment["export"].as_str().unwrap()));
        }
    }
    assert!(html.contains(&format!(">{helper_value}</span>")), "{html}");
    assert!(
        html.contains(&format!(">{child_value}</p>"))
            || html.contains(&format!(">{child_value}</strong>")),
        "{html}"
    );
    execute_fragments(&runtime, root, helper_value, child_value);
    runtime
}

fn execute_fragments(runtime: &Value, root: &Path, helper_value: i32, child_value: i32) {
    let node = std::env::var_os("NODE").unwrap_or_else(|| "node".into());
    if !Command::new(&node)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let directory = root.join("generated/node-acceptance");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("package.json"), "{\"type\":\"module\"}").unwrap();
    fs::write(directory.join("rsjs-runtime.js"), r#"
export function signal(initial) { let value = initial; return { get() { return value; }, set(next) { value = next; }, update(f) { value = f(value); }, subscribe() { return () => {}; } }; }
export const integer_signal = signal;
export function computed(read) { return { get: read, subscribe() { return () => {}; }, _dispose() {} }; }
export function read_signal_initials() { return []; }
export function signal_initial(initials, index, fallback) { return initials[index] ?? fallback; }
export const signal_initial_integer = signal_initial;
export function checked_int_add(a, b) { return a + b; }
export function checked_int_mul(a, b) { return a * b; }
export function walk_path() { return {}; }
export function bind_text(_node, read) { globalThis.values.push(read()); return () => {}; }
export function bind_conditional() { return () => {}; }
export function dispose_all(disposers) { while (disposers.length) disposers.pop()(); }
"#).unwrap();
    let mut script = String::new();
    for name in ["OwnerA", "OwnerB", "Child"] {
        let fragment = fragment(runtime, name);
        fs::write(
            directory.join(format!("{name}.js")),
            fragment["js"]
                .as_str()
                .unwrap()
                .replace("from \"/rsjs-runtime.js\"", "from \"./rsjs-runtime.js\""),
        )
        .unwrap();
        let expected = if name == "Child" {
            child_value
        } else {
            helper_value
        };
        script.push_str(&format!("{{ globalThis.values = []; const module = await import('./{name}.js'); const dispose = module[{}]({{}}, {{ signalSeeds: {{ 0: 1 }}, signals: {{ 0: {{ get() {{ return 1; }} }} }}, expose: {{ signals: {{}}, payloadHandlers: {{}}, lifecycleControls: {{}} }} }}); if (!values.includes({expected})) throw new Error('{name} wrong dynamic value: ' + JSON.stringify(values)); if (typeof dispose !== 'function') throw new Error('missing disposer'); dispose(); }}\n", serde_json::to_string(&fragment["export"]).unwrap()));
    }
    fs::write(directory.join("acceptance.mjs"), script).unwrap();
    let output = Command::new(&node)
        .current_dir(&directory)
        .arg("acceptance.mjs")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
