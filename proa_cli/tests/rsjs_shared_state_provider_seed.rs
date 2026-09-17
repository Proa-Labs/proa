// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for `SharedStateProvider` seed timing.
//!
//! The static half exercises descriptor compilation, exact relocation linking,
//! semantic planning, final Rust compilation, and the resulting executable.
//! The live half proves that the same imported component field cannot silently
//! become a one-time provider snapshot when a parent supplies a signal read.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{
    run_linked_cargo, DescriptorRootSelector, LinkedCargoError, LinkedCargoOutput,
    LinkedCargoRequest,
};
use proa_cli::rsjs::semantic_link::SemanticLinkError;
use rsjs_ir::abi::{
    AbiVersion, ComponentBindingSource, ComponentSemanticKey, DescriptorBody, InvocationId,
    ProducerVersion, TemporalPropMode,
};
use rsjs_ir::build::ArtifactRole;
use rsjs_ir::link::ComponentGraphLinkError;
use tempfile::TempDir;

const STATIC_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

mod state {
    #[derive(rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.provider-seed-state")]
    pub struct State {
        pub enabled: bool,
    }
}

mod child {
    use super::*;

    pub struct StaticChild {}

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticChild {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-static-child>"child inside provider"</span>
            }
            .render(cx)
        }
    }
}

mod provider {
    use super::*;
    use crate::child::StaticChild;
    use crate::state::State;
    use proa_core::WebRenderSync as RenderSync;

    pub struct ProviderPanel {
        pub enabled: bool,
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for ProviderPanel {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <section data-provider-panel>
                    {rsjs::SharedStateProvider {
                        state: State {
                            enabled: self.enabled,
                        },
                        children: StaticChild {},
                    }}
                </section>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::ProviderPanel as ExportedProvider;
    }
}

mod page {
    use super::*;
    use crate::provider::public::ExportedProvider as ImportedProvider;
    use proa_core::WebRenderSync as RenderSync;

    pub struct StaticPage;

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for StaticPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <main data-static-page>
                    {ImportedProvider {
                        enabled: true,
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
    let html = proa_core::render_to_string(page::StaticPage);
    assert!(
        html.contains("<main data-static-page>")
            && html.contains("<section data-provider-panel>")
            && html.contains("<span data-static-child>child inside provider</span>"),
        "the imported participant must render its child inside the wrapperless provider: {html}"
    );
    assert!(
        !html.contains("<rsjs-island")
            && !html.contains("data-rsjs-island-root")
            && !html.contains("rsjs-shared-state-seed-table")
            && !html.contains("<script"),
        "a static provider declaration must not publish an owner, client runtime, or seed table: {html}"
    );
    assert!(
        rsjs::manifest::manifest().islands.is_empty(),
        "the linked static page must not publish any owner asset"
    );

    print!("{html}");
}
"#;

const LIVE_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

mod state {
    #[derive(rsjs::SharedState)]
    #[rsjs(type_key = "acceptance.provider-seed-state")]
    pub struct State {
        pub enabled: bool,
    }
}

mod child {
    use super::*;

    pub struct StaticChild {}

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StaticChild {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-static-child>"child inside provider"</span>
            }
            .render(cx)
        }
    }
}

mod provider {
    use super::*;
    use crate::child::StaticChild;
    use crate::state::State;
    use proa_core::WebRenderSync as RenderSync;

    pub struct ProviderPanel {
        pub enabled: bool,
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for ProviderPanel {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <section data-provider-panel>
                    {rsjs::SharedStateProvider {
                        state: State {
                            enabled: self.enabled,
                        },
                        children: StaticChild {},
                    }}
                </section>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::ProviderPanel as ExportedProvider;
    }
}

mod page {
    use super::*;
    use crate::provider::public::ExportedProvider as ImportedProvider;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LivePage;

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for LivePage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let enabled = rsjs::signal(true);
            let toggle =
                rsjs::event_handler(|_: rsjs::Event| enabled.update(|value| !value));

            proa_macros::html_sync! {
                <main>
                    {ImportedProvider {
                        enabled: enabled.get(),
                    }}
                    <button type="button" on_click={toggle()}>
                        "Toggle provider seed"
                    </button>
                </main>
            }
            .render(cx)
        }
    }
}

fn main() {}
"#;

#[test]
fn shared_state_provider_accepts_static_seed_but_rejects_the_same_live_edge() {
    let static_temp = write_fixture(
        "rsjs_shared_state_provider_static_seed_fixture",
        STATIC_FIXTURE,
    );
    let static_output = run_fixture(
        &static_temp,
        "rsjs_shared_state_provider_static_seed_fixture",
        "rsjs-shared-state-provider-static-seed",
    )
    .expect("a static server value may initialize SharedStateProvider");

    assert_static_provider_plan(&static_output);

    let executable = static_output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final static-provider fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(
        html.contains("data-static-page")
            && html.contains("data-provider-panel")
            && html.contains("<span data-static-child>child inside provider</span>")
            && !html.contains("<rsjs-island")
            && !html.contains("rsjs-shared-state-seed-table")
            && !html.contains("<script"),
        "the final executable must preserve inert wrapperless provider rendering: {html}"
    );

    let live_temp = write_fixture("rsjs_shared_state_provider_live_seed_fixture", LIVE_FIXTURE);
    let error = run_fixture(
        &live_temp,
        "rsjs_shared_state_provider_live_seed_fixture",
        "rsjs-shared-state-provider-live-seed",
    )
    .expect_err("a live parent value must not become a provider initialization snapshot");
    assert!(
        matches!(
            &error,
            LinkedCargoError::SemanticLink(SemanticLinkError::ComponentGraph(
                ComponentGraphLinkError::ForbiddenLiveProviderSeed {
                    field_name,
                    ..
                }
            )) if field_name == "enabled"
        ),
        "expected the canonical forbidden-provider-seed error, got: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(
            "passes live field \"enabled\" into SharedStateProvider initialization"
        ) && message.contains(
            "initialize the provider from static server data or make SharedState the source of truth"
        ),
        "the linked diagnostic must explain the temporal contract and remediation: {message}"
    );
}

fn assert_static_provider_plan(output: &LinkedCargoOutput) {
    assert_eq!(
        output.transport.shared_state_definitions.len(),
        1,
        "the derived bool schema must resolve into one transport definition"
    );
    let schema = &output.transport.shared_state_definitions[0];
    assert_eq!(schema.type_key, "acceptance.provider-seed-state");
    assert!(
        !schema.schema_abi.is_empty(),
        "the fixture must carry a complete resolved canonical schema ABI"
    );
    assert!(
        output
            .transport
            .components
            .iter()
            .all(|component| component.shared_state_acquisitions.is_empty()),
        "declaring a provider may use the shared-state relocation record family \
         for topology, but no component may claim an rsjs::shared::<T>() acquisition"
    );

    let plan = &output.semantic;
    assert!(
        plan.owners.owners.is_empty() && plan.owners.candidate_to_owner.is_empty(),
        "the static page and participant must not form a browser owner"
    );
    assert!(
        plan.component_inputs.live_props.is_empty()
            && plan.component_inputs.live_callbacks.is_empty(),
        "a static provider seed must remain an ordinary server edge"
    );
    assert!(
        plan.shared_state.is_empty(),
        "without rsjs::shared::<T>(), no browser SharedState schema is reachable"
    );

    let page_key = component_key(
        "rsjs_shared_state_provider_static_seed_fixture",
        "StaticPage",
        ["page"],
    );
    let provider_key = component_key(
        "rsjs_shared_state_provider_static_seed_fixture",
        "ProviderPanel",
        ["provider"],
    );
    let page = plan
        .components
        .iter()
        .find(|component| component.semantic_key == page_key)
        .expect("canonical static page");
    let DescriptorBody::Component(page_descriptor) = &page.descriptor.body else {
        panic!("static page descriptor must be a component");
    };
    let provider_edge = page_descriptor
        .direct_components
        .values()
        .next()
        .expect("imported provider participant edge");
    assert!(matches!(
        provider_edge
            .bindings
            .get("enabled")
            .map(|binding| &binding.source),
        Some(ComponentBindingSource::ServerOnly)
    ));
    assert_eq!(
        page.component_edges
            .iter()
            .map(|edge| &edge.target)
            .collect::<Vec<_>>(),
        vec![&provider_key],
        "the imported and renamed syntax must resolve to the defining component"
    );

    let provider = plan
        .components
        .iter()
        .find(|component| component.semantic_key == provider_key)
        .expect("canonical provider participant");
    let DescriptorBody::Component(provider_descriptor) = &provider.descriptor.body else {
        panic!("provider descriptor must be a component");
    };
    assert!(
        provider_descriptor
            .prop_uses
            .values()
            .any(|prop| prop.temporal == TemporalPropMode::ForbiddenProviderSeed),
        "the participant must retain a latent rejection for a future live caller"
    );
    assert!(
        provider_descriptor.effects.is_empty() && provider_descriptor.shared_state.is_empty(),
        "provider declaration alone must not originate browser work or acquire SharedState"
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
