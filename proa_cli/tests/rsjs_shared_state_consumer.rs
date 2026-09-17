// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for relocation-resolved SharedState
//! consumers.
//!
//! The positive fixture exercises descriptor compilation, exact relocation
//! linking, canonical graph lowering, semantic owner planning, final Rust
//! compilation, and the resulting executable. The state type and consumer are
//! both imported through ordinary Rust aliases so no path-token spelling can
//! become protocol authority.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, DescriptorRootSelector, LinkedCargoError, LinkedCargoOutput,
    LinkedCargoRequest,
};
use proa_cli::rsjs::semantic_link::{
    SemanticLinkError, StandaloneContextRequirement, StandaloneRootDisposition,
};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, DescriptorBody, EffectKind, InvocationId, OriginKind,
    ProducerVersion, SemanticHash, SharedStateRefId,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

static FIXTURE_BUILD_LOCK: Mutex<()> = Mutex::new(());

const CONSUMER_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{
    Ctx, DataLoader, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use rsjs::rsjs;

mod state_definition {
    #[derive(Clone, Debug, rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.shared-state-consumer")]
    pub struct WorkbenchState {
        pub count: u32,
        pub status: String,
    }

    pub mod public {
        pub use super::WorkbenchState as ExportedWorkbenchState;
    }
}

mod participant {
    use super::*;
    use crate::state_definition::public::ExportedWorkbenchState as StateAlias;
    use proa_core::WebRenderSync as RenderSync;

    pub struct SharedCounter {}

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for SharedCounter {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let workbench = rsjs::shared::<StateAlias>();
            let count = workbench.count();
            let status = workbench.status();
            let increment =
                rsjs::event_handler(|_: rsjs::Event| count.update(|value| value + 1_u32));
            let reset = rsjs::event_handler(|_: rsjs::Event| {
                count.set(0_u32);
                status.set("reset".to_string());
            });

            proa_macros::html_sync! {
                <section data-shared-counter>
                    <output data-count>{count.get()}</output>
                    <span data-status>{status.get()}</span>
                    <button type="button" on_click={increment()}>
                        "Increment"
                    </button>
                    <button type="button" on_click={reset()}>
                        "Reset"
                    </button>
                </section>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::SharedCounter as ExportedCounter;
    }
}

mod page {
    use super::*;
    use crate::participant::public::ExportedCounter as ImportedCounter;
    use crate::state_definition::public::ExportedWorkbenchState as ProviderStateAlias;
    use proa_core::WebRenderSync as RenderSync;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct WorkbenchPage;

    static PROVIDER_SEED_EVALUATIONS: AtomicUsize = AtomicUsize::new(0);

    fn provider_status() -> String {
        PROVIDER_SEED_EVALUATIONS.fetch_add(1, Ordering::SeqCst);
        "ready".to_owned()
    }

    pub fn provider_seed_evaluations() -> usize {
        PROVIDER_SEED_EVALUATIONS.load(Ordering::SeqCst)
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for WorkbenchPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <main data-workbench-page>
                    {rsjs::SharedStateProvider {
                        state: ProviderStateAlias {
                            count: 5_u32,
                            status: provider_status(),
                        },
                        children: ImportedCounter {},
                    }}
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
    let (seed_table, out) = {
        let mut cx = Ctx::<NoLoader, Vec<u8>>::with_buffer(Vec::new());
        page::WorkbenchPage
            .render(&mut cx)
            .expect("SharedState consumer page renders");
        let seed_table = cx
            .rsjs_shared_state_seed_table_json()
            .expect("a committed provider with a live consumer publishes one seed");
        (seed_table, cx.into_output())
    };
    let html = String::from_utf8(out).expect("rendered HTML is UTF-8");
    let runtime_schema_hash =
        <state_definition::WorkbenchState as rsjs::SharedState>::schema_hash();

    assert_eq!(
        page::provider_seed_evaluations(),
        1,
        "the final provider rewrite must evaluate the authored state expression exactly once"
    );
    assert_eq!(
        html.matches("<rsjs-island").count(),
        1,
        "the imported SharedState consumer must be the only browser owner: {html}"
    );
    assert!(
        html.contains("data-workbench-page")
            && html.contains("data-shared-counter")
            && html.contains("<output data-count")
            && html.contains(">5</output>")
            && html.contains("<span data-status")
            && html.contains(">ready</span>")
            && html.contains("data-rsjs-handler=")
            && html.contains("data-rsjs-bind="),
        "the consumer must SSR from the provider snapshot and retain its bindings: {html}"
    );
    assert!(
        html.contains("\"shared\":[{\"scope_id\":0")
            && html.contains("\"type_key\":\"acceptance.shared-state-consumer\"")
            && html.contains(&format!("\"schema_hash\":\"{runtime_schema_hash}\"")),
        "the adjacent owner metadata must bind to the exact provider scope and schema: {html}"
    );
    assert!(
        !html.contains("\"signals\":[5]")
            && !html.contains("\"signals\":[\"ready\"]"),
        "page-owned SharedState fields must not be duplicated as local signal seeds: {html}"
    );
    assert!(
        seed_table.contains("\"scope_id\":0")
            && seed_table.contains("\"type_key\":\"acceptance.shared-state-consumer\"")
            && seed_table.contains(&format!("\"schema_hash\":\"{runtime_schema_hash}\""))
            && seed_table.contains("\"count\":5")
            && seed_table.contains("\"status\":\"ready\""),
        "the response seed table must carry the provider state exactly once: {seed_table}"
    );

    let manifest = rsjs::manifest::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "SharedCounter")
        .expect("SharedCounter fragment");
    assert!(
        fragment
            .js_source
            .contains("acquire_shared_state(options.seedRoot ?? root, 0)")
            && fragment
                .js_source
                .contains(".field(\"count\")")
            && fragment
                .js_source
                .contains(".field(\"status\")")
            && fragment
                .js_source
                .contains("bind_delegated_events(root, \"click\""),
        "the fragment must acquire the adjacent provider scope and retain reads/writes:\n{}",
        fragment.js_source
    );
    assert!(
        !fragment.js_source.contains("read_signal_initials("),
        "SharedState fields must not become component-local signal seeds:\n{}",
        fragment.js_source
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
        "exactly one linked owner must activate for the rendered consumer"
    );
    assert!(
        rendered_owners[0]
            .component_fragment_imports
            .contains(&fragment.asset_id),
        "the rendered owner must import the SharedCounter fragment"
    );

    print!("{html}");
}
"#;

const CONFLICT_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

mod first {
    use super::*;

    #[derive(Clone, Debug, rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.shared-state-conflict")]
    pub struct State {
        pub value: u32,
    }

    pub struct Consumer {}

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Consumer {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let state = rsjs::shared::<State>();
            let value = state.value();
            let clear =
                rsjs::event_handler(|_: rsjs::Event| value.set(0_u32));

            proa_macros::html_sync! {
                <button type="button" on_click={clear()}>{value.get()}</button>
            }
            .render(cx)
        }
    }
}

mod second {
    use super::*;

    #[derive(Clone, Debug, rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.shared-state-conflict")]
    pub struct State {
        pub value: String,
    }

    pub struct Consumer {}

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Consumer {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let state = rsjs::shared::<State>();
            let value = state.value();
            let replace =
                rsjs::event_handler(|_: rsjs::Event| value.set("changed".to_string()));

            proa_macros::html_sync! {
                <button type="button" on_click={replace()}>{value.get()}</button>
            }
            .render(cx)
        }
    }
}

pub struct ConflictPage;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ConflictPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <main>
                {rsjs::SharedStateProvider {
                    state: first::State {
                        value: 1_u32,
                    },
                    children: first::Consumer {},
                }}
                {rsjs::SharedStateProvider {
                    state: second::State {
                        value: "second".to_owned(),
                    },
                    children: second::Consumer {},
                }}
            </main>
        }
        .render(cx)
    }
}

fn main() {}
"#;

#[test]
fn linked_shared_state_consumer_uses_exact_schema_and_activates_one_owner() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = write_fixture("rsjs_shared_state_consumer_fixture", CONSUMER_FIXTURE);
    let output = run_fixture(
        &temp,
        "rsjs_shared_state_consumer_fixture",
        "rsjs-shared-state-consumer",
    )
    .expect("a resolved SharedState consumer must link and compile");

    assert_transport_identity(&output);
    assert_canonical_consumer(&output);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final SharedState consumer executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(
        html.contains("data-shared-counter")
            && html.contains(">5</output>")
            && html.contains(">ready</span>")
            && html.contains("\"shared\":[{\"scope_id\":0"),
        "the final executable must preserve provider-to-consumer identity: {html}"
    );
}

#[test]
fn same_shared_state_type_key_with_different_exact_schemas_is_rejected() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = write_fixture("rsjs_shared_state_conflict_fixture", CONFLICT_FIXTURE);
    let error = run_fixture(
        &temp,
        "rsjs_shared_state_conflict_fixture",
        "rsjs-shared-state-conflict",
    )
    .expect_err("one public type_key cannot select two different wire schemas");

    let LinkedCargoError::SemanticLink(SemanticLinkError::SharedStateDeclarationConflict {
        type_key,
        first_schema,
        second_schema,
        ..
    }) = &error
    else {
        panic!("expected an exact linked SharedState schema conflict, got: {error:?}");
    };
    assert_eq!(type_key, "acceptance.shared-state-conflict");
    assert_ne!(first_schema, second_schema);
    let message = error.to_string();
    assert!(
        message.contains("acceptance.shared-state-conflict")
            && message.contains("incompatible schema ABIs")
            && message.contains("before any owner activates"),
        "the error must explain the page-wide schema identity contract: {message}"
    );
}

fn assert_transport_identity(output: &LinkedCargoOutput) {
    let component = output
        .transport
        .components
        .iter()
        .find(|component| component.identity.item == "SharedCounter")
        .expect("transport SharedCounter definition");
    assert_eq!(
        component.identity.module_path,
        ["participant"],
        "the definition identity must be the defining module, not an import alias"
    );
    assert_eq!(
        component.shared_state_acquisitions.len(),
        1,
        "the reachable shared::<Alias>() call owns one exact acquisition"
    );
    let claim = &component.shared_state_acquisitions[0];
    assert_eq!(claim.store_id, 0);

    let acquisition = output
        .transport
        .shared_state_acquisitions
        .iter()
        .find(|acquisition| acquisition.source_edge == claim.source_edge)
        .expect("global exact acquisition record");
    assert_eq!(
        acquisition.target, claim.target,
        "the component claim and relocation-resolved global record must select the same definition"
    );
    let definition = output
        .transport
        .shared_state_definitions
        .iter()
        .find(|definition| definition.definition == claim.target)
        .expect("relocation-selected SharedState definition");
    assert_eq!(definition.type_key, "acceptance.shared-state-consumer");
    assert!(
        !definition.schema_abi.is_empty(),
        "the selected definition must carry complete canonical schema bytes"
    );
}

fn assert_canonical_consumer(output: &LinkedCargoOutput) {
    let plan = &output.semantic;
    let consumer_key = component_key(
        "rsjs_shared_state_consumer_fixture",
        "SharedCounter",
        ["participant"],
    );
    let component = plan
        .components
        .iter()
        .find(|component| component.semantic_key == consumer_key)
        .expect("canonical SharedCounter");
    let DescriptorBody::Component(body) = &component.descriptor.body else {
        panic!("SharedCounter descriptor must be a component");
    };
    assert_eq!(
        body.shared_state.len(),
        1,
        "one acquired store must become one canonical SharedState reference"
    );
    let reference = body
        .shared_state
        .get(&SharedStateRefId::new(0))
        .expect("dense SharedState reference zero");
    assert_eq!(reference.type_key, "acceptance.shared-state-consumer");
    assert_eq!(
        body.origins.get(&reference.origin),
        Some(&OriginKind::SharedState),
        "the relocation-selected acquisition is the canonical browser origin"
    );
    assert!(
        body.effects
            .values()
            .any(|effect| *effect == EffectKind::SharedStateRead),
        "rendered field values must lower to a SharedState read effect"
    );
    assert!(
        body.effects
            .values()
            .any(|effect| *effect == EffectKind::SharedStateWrite),
        "bound set/update handlers must lower to a SharedState write effect"
    );

    let transport_component = output
        .transport
        .components
        .iter()
        .find(|component| component.identity.item == "SharedCounter")
        .unwrap();
    let target = transport_component.shared_state_acquisitions[0].target;
    let definition = output
        .transport
        .shared_state_definitions
        .iter()
        .find(|definition| definition.definition == target)
        .unwrap();
    assert_eq!(
        reference.schema_abi,
        SemanticHash::from_bytes(definition.schema_abi_hash),
        "canonical identity must use the transport-domain hash of the exact \
         relocation-selected schema-ABI bytes"
    );

    let consumer_sites = plan
        .sites
        .iter()
        .filter(|site| site.component == consumer_key)
        .collect::<Vec<_>>();
    assert!(
        !consumer_sites.is_empty() && consumer_sites.iter().all(|site| site.parent.is_some()),
        "the provider-dependent consumer must exist only at admitted lexical sites"
    );
    let root_decision = plan
        .root_decisions
        .iter()
        .find(|decision| decision.component == consumer_key)
        .expect("standalone-root decision for SharedCounter");
    assert!(
        matches!(
            &root_decision.disposition,
            StandaloneRootDisposition::ContextRequired { missing }
                if matches!(
                    missing.as_slice(),
                    [StandaloneContextRequirement::SharedState(requirement)]
                        if requirement.reference == SharedStateRefId::new(0)
                            && requirement.type_key == "acceptance.shared-state-consumer"
                )
        ),
        "the standalone consumer must be withheld with its exact missing provider requirement"
    );
    let expected_candidates = consumer_sites
        .iter()
        .map(|site| site.candidate)
        .collect::<BTreeSet<_>>();
    assert!(
        expected_candidates
            .iter()
            .all(|candidate| plan.owners.candidate_to_owner.contains_key(candidate)),
        "every active SharedCounter occurrence must belong to a linked owner"
    );
    let linked_schema = plan
        .shared_state
        .iter()
        .find(|schema| schema.type_key == "acceptance.shared-state-consumer")
        .expect("linked page-level SharedState schema");
    assert_eq!(linked_schema.schema_abi, reference.schema_abi);
    assert_eq!(
        linked_schema.consumers.len(),
        expected_candidates.len(),
        "every instantiated SharedCounter acquisition contributes one schema consumer"
    );
    let actual_candidates = linked_schema
        .consumers
        .iter()
        .map(|consumer| consumer.component)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_candidates, expected_candidates,
        "schema consumers must name the exact instantiated SharedCounter sites"
    );
    assert_eq!(
        plan.shared_state_providers.len(),
        1,
        "the admitted page contains one exact lexical provider occurrence"
    );
    assert_eq!(
        plan.shared_state_requirements.len(),
        expected_candidates.len(),
        "every admitted consumer occurrence must resolve one exact provider requirement"
    );
    let provider = &plan.shared_state_providers[0];
    assert_eq!(provider.type_key, "acceptance.shared-state-consumer");
    assert!(
        plan.shared_state_requirements.iter().all(|requirement| {
            expected_candidates.contains(&requirement.consumer)
                && requirement.reference == SharedStateRefId::new(0)
                && requirement.provider_host == provider.host
                && requirement.provider_site == provider.provider_site
                && requirement.type_key == provider.type_key
                && requirement.schema_abi == provider.schema_abi
        }),
        "linked acquisitions must target the exact provider occurrence with no nearest-key fallback"
    );
    assert!(
        linked_schema.consumers.iter().all(|consumer| {
            consumer.reference == SharedStateRefId::new(0)
                && consumer.origin == reference.origin
                && plan
                    .owners
                    .candidate_to_owner
                    .get(&consumer.component)
                    .is_some_and(|owner| {
                        plan.owners
                            .owners
                            .iter()
                            .any(|partition| partition.id == *owner)
                    })
        }),
        "each schema consumer must retain the exact local reference/origin and a published owner"
    );
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

fn run_fixture(
    temp: &TempDir,
    package: &str,
    invocation: &str,
) -> Result<LinkedCargoOutput, LinkedCargoError> {
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
