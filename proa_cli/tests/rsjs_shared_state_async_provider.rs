// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for an exact SharedState provider across an
//! authored asynchronous render boundary.
//!
//! The final executable deliberately installs a nearer, unlinked provider with
//! the same protocol key after the consumer has returned `Poll::Pending`.
//! Exact linked acquisition must ignore that ambient binding, keep the
//! compiler-selected provider scope alive across suspension, SSR the selected
//! value, and publish only the selected provider's seed.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, DescriptorRootSelector, LinkedCargoOutput, LinkedCargoRequest,
};
use proa_cli::rsjs::semantic_link::{StandaloneContextRequirement, StandaloneRootDisposition};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, DescriptorBody, InvocationId, ProducerVersion,
    SharedStateRefId,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

static FIXTURE_BUILD_LOCK: Mutex<()> = Mutex::new(());

const ASYNC_PROVIDER_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use proa_core::{
    Ctx, DataLoader, NoLoader, RenderOutcome, WebContext, WebRender, WriteBuf, WriteError,
};
use rsjs::rsjs;

mod state_definition {
    #[derive(Clone, Debug, rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.async-shared-state-provider")]
    pub struct AsyncWorkbenchState {
        pub count: u32,
        pub marker: u32,
    }

    pub mod public {
        pub use super::AsyncWorkbenchState as RenamedWorkbenchState;
    }
}

mod async_consumer {
    use super::*;
    use crate::state_definition::public::RenamedWorkbenchState as ConsumerStateAlias;
    use proa_core::WebRender as RenderAsync;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct AsyncSharedCounter {}

    static PENDING_POLLS: AtomicUsize = AtomicUsize::new(0);
    static READY_POLLS: AtomicUsize = AtomicUsize::new(0);

    struct YieldOnce {
        yielded: bool,
    }

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.yielded {
                READY_POLLS.fetch_add(1, Ordering::SeqCst);
                Poll::Ready(())
            } else {
                self.yielded = true;
                PENDING_POLLS.fetch_add(1, Ordering::SeqCst);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    pub fn poll_counts() -> (usize, usize) {
        (
            PENDING_POLLS.load(Ordering::SeqCst),
            READY_POLLS.load(Ordering::SeqCst),
        )
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for AsyncSharedCounter {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                // The provider scope and its exact linked tag must remain live
                // while this future is suspended.
                YieldOnce { yielded: false }.await;

                // Install a nearer provider with the same wire schema. The
                // generated linked acquisition below must not use nearest-key
                // fallback, so this ambient state must remain inert.
                let ambient_state = ConsumerStateAlias {
                    count: 99_u32,
                    marker: 100_u32,
                };
                let ambient_json = rsjs::ClientEncode::encode(&ambient_state);
                let _ambient_scope = cx.rsjs_shared_state_enter(
                    <ConsumerStateAlias as rsjs::SharedState>::TYPE_KEY,
                    <ConsumerStateAlias as rsjs::SharedState>::schema_hash(),
                    <ConsumerStateAlias as rsjs::SharedState>::META.rust_type,
                    &[],
                    Rc::new(ambient_state),
                    ambient_json,
                )?;

                let workbench = rsjs::shared::<ConsumerStateAlias>();
                let count = workbench.count();
                let marker = workbench.marker();
                let increment =
                    rsjs::event_handler(|_: rsjs::Event| count.update(|value| value + 1_u32));

                proa_macros::html! {
                    <section
                        data-async-shared-counter
                        data_count={count.get()}
                        data_marker={marker.get()}
                    >
                        <button type="button" on_click={increment()}>
                            "Increment"
                        </button>
                    </section>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }

    pub mod public {
        pub use super::AsyncSharedCounter as RenamedAsyncCounter;
    }
}

mod async_page {
    use super::*;
    use crate::async_consumer::public::RenamedAsyncCounter as ImportedAsyncCounter;
    use crate::state_definition::public::RenamedWorkbenchState as ProviderStateAlias;
    use proa_core::WebRender as RenderAsync;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct AsyncWorkbenchPage;

    static PROVIDER_SEED_EVALUATIONS: AtomicUsize = AtomicUsize::new(0);

    fn provider_marker() -> u32 {
        PROVIDER_SEED_EVALUATIONS.fetch_add(1, Ordering::SeqCst);
        42_u32
    }

    pub fn provider_seed_evaluations() -> usize {
        PROVIDER_SEED_EVALUATIONS.load(Ordering::SeqCst)
    }

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for AsyncWorkbenchPage {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                proa_macros::html! {
                    <main data-async-provider-page>
                        {rsjs::SharedStateProvider {
                            state: ProviderStateAlias {
                                count: 7_u32,
                                marker: provider_marker(),
                            },
                            children: ImportedAsyncCounter {},
                        }}
                    </main>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

#[cfg(not(rsjs_final))]
fn main() {}

#[cfg(rsjs_final)]
fn main() {
    let (seed_table, out) = {
        let mut cx = Ctx::<NoLoader, Vec<u8>>::with_buffer(Vec::new());
        block_on(
            async_page::AsyncWorkbenchPage
                .render(&mut cx)
                .resolve(),
        )
        .expect("the linked async SharedState page renders");
        let seed_table = cx
            .rsjs_shared_state_seed_table_json()
            .expect("the exact committed provider publishes one seed");
        (seed_table, cx.into_output())
    };
    let html = String::from_utf8(out).expect("rendered HTML is UTF-8");
    let runtime_schema_hash =
        <state_definition::AsyncWorkbenchState as rsjs::SharedState>::schema_hash();

    assert_eq!(
        async_page::provider_seed_evaluations(),
        1,
        "the provider state expression must be evaluated exactly once"
    );
    assert_eq!(
        async_consumer::poll_counts(),
        (1, 1),
        "the consumer must suspend once and resume once under the linked provider scope"
    );
    assert_eq!(
        html.matches("<rsjs-island").count(),
        1,
        "the provider-dependent async consumer must activate exactly one owner: {html}"
    );
    assert!(
        html.contains("data-async-provider-page")
            && html.contains("data-async-shared-counter")
            && html.contains("data-count=\"7\"")
            && html.contains("data-marker=\"42\"")
            && html.contains("data-rsjs-handler=")
            && html.contains("data-rsjs-attr="),
        "the async child must SSR from the exact linked provider: {html}"
    );
    assert!(
        !html.contains("data-count=\"99\"") && !html.contains("data-marker=\"100\""),
        "the linked acquisition must not fall back to the nearer ambient provider: {html}"
    );
    assert!(
        !html.contains("<rsjs-shared-state-provider")
            && !html.contains("<shared-state-provider")
            && !html.contains("data-rsjs-provider"),
        "SharedStateProvider must remain wrapperless: {html}"
    );
    assert!(
        html.contains("\"shared\":[{\"scope_id\":0")
            && html.contains("\"type_key\":\"acceptance.async-shared-state-provider\"")
            && html.contains(&format!("\"schema_hash\":\"{runtime_schema_hash}\"")),
        "the adjacent owner metadata must name the exact linked provider scope: {html}"
    );
    assert_eq!(
        seed_table.matches("\"type_key\":").count(),
        1,
        "only the selected provider may publish a seed: {seed_table}"
    );
    assert!(
        seed_table.contains("\"scope_id\":0")
            && seed_table.contains("\"type_key\":\"acceptance.async-shared-state-provider\"")
            && seed_table.contains(&format!("\"schema_hash\":\"{runtime_schema_hash}\""))
            && seed_table.contains("\"count\":7")
            && seed_table.contains("\"marker\":42")
            && !seed_table.contains("\"count\":99")
            && !seed_table.contains("\"marker\":100"),
        "the seed table must contain only the exact linked provider snapshot: {seed_table}"
    );

    let manifest = rsjs::manifest::manifest();
    let fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "AsyncSharedCounter")
        .expect("AsyncSharedCounter fragment");
    assert!(
        fragment
            .js_source
            .contains("acquire_shared_state(options.seedRoot ?? root, 0)")
            && fragment.js_source.contains(".field(\"count\")")
            && fragment.js_source.contains(".field(\"marker\")")
            && fragment
                .js_source
                .contains("bind_delegated_events(root, \"click\""),
        "the async child fragment must retain exact SharedState reads/writes:\n{}",
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
        "the final executable must activate one linked owner"
    );
    assert!(
        rendered_owners[0]
            .component_fragment_imports
            .contains(&fragment.asset_id),
        "the rendered owner must import the async SharedState consumer fragment"
    );

    print!("{html}");
}
"#;

#[test]
fn exact_async_provider_survives_pending_and_rejects_ambient_fallback() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = write_fixture(
        "rsjs_shared_state_async_provider_fixture",
        ASYNC_PROVIDER_FIXTURE,
    );
    let output = run_fixture(
        &temp,
        "rsjs_shared_state_async_provider_fixture",
        "rsjs-shared-state-async-provider",
    );

    assert_exact_async_provider_plan(&output);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final async SharedState provider executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(
        html.contains("data-async-shared-counter")
            && html.contains("data-count=\"7\"")
            && html.contains("data-marker=\"42\"")
            && !html.contains("data-count=\"99\"")
            && !html.contains("data-marker=\"100\""),
        "the executable must use the exact provider after an actual async suspension: {html}"
    );
}

fn assert_exact_async_provider_plan(output: &LinkedCargoOutput) {
    let transport_page = output
        .transport
        .components
        .iter()
        .find(|component| component.identity.item == "AsyncWorkbenchPage")
        .expect("transport async provider page");
    assert_eq!(
        transport_page.identity.module_path,
        ["async_page"],
        "the page identity must come from its defining module, not its imports"
    );
    assert_eq!(
        transport_page.shared_state_provider_sites.len(),
        1,
        "the imported provider state and child aliases must resolve one authenticated site"
    );
    let transport_consumer = output
        .transport
        .components
        .iter()
        .find(|component| component.identity.item == "AsyncSharedCounter")
        .expect("transport async SharedState consumer");
    assert_eq!(
        transport_consumer.identity.module_path,
        ["async_consumer"],
        "the child identity must come from its defining module, not its renamed import"
    );
    assert_eq!(transport_consumer.shared_state_acquisitions.len(), 1);
    let transport_provider = &transport_page.shared_state_provider_sites[0];
    assert_eq!(
        transport_provider.child_target, transport_consumer.definition,
        "rustc must resolve the renamed direct child to its exact component definition"
    );
    assert_eq!(
        transport_provider.target, transport_consumer.shared_state_acquisitions[0].target,
        "the renamed provider state and consumer state aliases must select one exact ABI definition"
    );
    let transport_state = output
        .transport
        .shared_state_definitions
        .iter()
        .find(|definition| definition.definition == transport_provider.target)
        .expect("transport SharedState definition selected through both aliases");
    assert_eq!(
        transport_state.type_key,
        "acceptance.async-shared-state-provider"
    );

    let plan = &output.semantic;
    let consumer_key = component_key(
        "rsjs_shared_state_async_provider_fixture",
        "AsyncSharedCounter",
        ["async_consumer"],
    );
    let page_key = component_key(
        "rsjs_shared_state_async_provider_fixture",
        "AsyncWorkbenchPage",
        ["async_page"],
    );

    let component = plan
        .components
        .iter()
        .find(|component| component.semantic_key == consumer_key)
        .expect("canonical async SharedState consumer");
    let DescriptorBody::Component(body) = &component.descriptor.body else {
        panic!("AsyncSharedCounter descriptor must be a component");
    };
    assert_eq!(
        body.shared_state.len(),
        1,
        "the async consumer must retain one exact SharedState acquisition"
    );
    let reference = body
        .shared_state
        .get(&SharedStateRefId::new(0))
        .expect("dense async SharedState reference zero");

    let root_decision = plan
        .root_decisions
        .iter()
        .find(|decision| decision.component == consumer_key)
        .expect("standalone-root decision for AsyncSharedCounter");
    assert!(
        matches!(
            &root_decision.disposition,
            StandaloneRootDisposition::ContextRequired { missing }
                if matches!(
                    missing.as_slice(),
                    [StandaloneContextRequirement::SharedState(requirement)]
                        if requirement.consumer == consumer_key
                            && requirement.reference == SharedStateRefId::new(0)
                            && requirement.type_key
                                == "acceptance.async-shared-state-provider"
                            && requirement.schema_abi == reference.schema_abi
                )
        ),
        "a provider-dependent async consumer must be withheld as a standalone root"
    );

    assert_eq!(
        plan.shared_state_providers.len(),
        1,
        "the admitted async page must contain one exact static provider occurrence"
    );
    assert_eq!(
        plan.shared_state_requirements.len(),
        1,
        "the admitted async consumer must resolve one exact provider requirement"
    );
    let provider = &plan.shared_state_providers[0];
    let requirement = &plan.shared_state_requirements[0];
    assert_eq!(
        requirement.consumer, provider.child,
        "the provider's direct child is the exact consuming occurrence"
    );
    assert_eq!(requirement.reference, SharedStateRefId::new(0));
    assert_eq!(requirement.provider_host, provider.host);
    assert_eq!(requirement.provider_site, provider.provider_site);
    assert_eq!(requirement.type_key, provider.type_key);
    assert_eq!(requirement.schema_abi, provider.schema_abi);
    assert_eq!(provider.schema_abi, reference.schema_abi);
    assert_eq!(
        provider.type_key, "acceptance.async-shared-state-provider",
        "renamed Rust imports must not replace the public protocol identity"
    );

    let host = plan
        .sites
        .iter()
        .find(|site| site.candidate == provider.host)
        .expect("provider host occurrence");
    assert_eq!(
        host.component, page_key,
        "the exact provider occurrence must belong to the imported-child page"
    );
    let child = plan
        .sites
        .iter()
        .find(|site| site.candidate == provider.child)
        .expect("provider child occurrence");
    assert_eq!(child.component, consumer_key);
    assert_eq!(child.parent, Some(provider.host));

    assert_eq!(
        plan.owners.owners.len(),
        1,
        "the async provider page must produce one browser owner"
    );
    assert_eq!(
        plan.owners.candidate_to_owner.get(&provider.child),
        Some(&plan.owners.owners[0].id),
        "the exact async consumer occurrence must belong to that owner"
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
    .expect("the exact async SharedState provider must link and compile")
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
