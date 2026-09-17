// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for static scalar and immutable exact-one
//! HTML inputs at explicit island boundaries.
//!
//! These tests exercise descriptor compilation, exact relocation linking,
//! semantic planning, final Rust compilation, and the resulting executable.
//! Analyzer-only assertions cannot prove that server-only input reaches the
//! independent child's SSR output without becoming an implicit parent
//! snapshot, serialized slot, or client-side reconstruction program.

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
use proa_cli::rsjs::semantic_link::SemanticLinkError;
use rsjs_ir::abi::{
    AbiVersion, ComponentBindingSource, ComponentBoundary, ComponentFieldCodec, ComponentFieldRole,
    ComponentSemanticKey, DescriptorBody, InvocationId, ProducerVersion, SpanCardinality,
    StaticSlotCapability, TemporalPropMode,
};
use rsjs_ir::build::ArtifactRole;
use rsjs_ir::link::ComponentGraphLinkError;
use tempfile::TempDir;

static FIXTURE_BUILD_LOCK: Mutex<()> = Mutex::new(());

const STATIC_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};
use rsjs::rsjs;

mod counter {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct IndependentCounter {
        pub initial: u32,
    }

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for IndependentCounter {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let local = rsjs::signal::<u32>(self.initial);
            let increment =
                rsjs::event_handler(|_: rsjs::Event| local.update(|value| value + 1));

            proa_macros::html_sync! {
                <button
                    type="button"
                    data-independent-counter
                    on_click={increment()}
                >
                    {local.get()}
                </button>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::IndependentCounter as ImportedCounter;
    }
}

mod shell {
    use super::*;
    use crate::counter::public::ImportedCounter;
    use proa_core::WebRenderSync as RenderSync;

    pub struct StaticShell {
        pub initial: u32,
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for StaticShell {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <main data-static-shell>
                    {rsjs::island(ImportedCounter {
                        initial: self.initial,
                    })}
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
    let html = proa_core::render_to_string(
        shell::StaticShell { initial: 41 },
    );
    assert_eq!(
        html.matches("<rsjs-island").count(),
        1,
        "the static parent must not become an owner, while its explicit child must: {html}"
    );
    assert!(
        html.contains("data-static-shell")
            && html.contains("data-independent-counter")
            && html.contains("data-rsjs-handler=")
            && html.contains("data-rsjs-bind=")
            && html.contains(">41</button>")
            && html.contains("\"signals\":[41]"),
        "the independent child must SSR and seed from the authored server scalar: {html}"
    );

    let manifest = rsjs::manifest::manifest();
    let child_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "IndependentCounter")
        .expect("independent child fragment");
    assert!(
        child_fragment.js_source.contains("integer_signal(")
            && child_fragment
                .js_source
                .contains("bind_delegated_events(root, \"click\""),
        "the child fragment must retain its local signal and activation binding:\n{}",
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
        "exactly one linked owner must activate for the rendered explicit child"
    );
    assert!(
        rendered_owners[0]
            .component_fragment_imports
            .contains(&child_fragment.asset_id),
        "the rendered owner must import the independently hydrated child fragment"
    );

    print!("{html}");
}
"#;

const LIVE_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};
use rsjs::rsjs;

mod counter {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct IndependentCounter {
        pub initial: u32,
    }

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for IndependentCounter {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let local = rsjs::signal::<u32>(self.initial);
            proa_macros::html_sync! {
                <button on_click={|_| local.update(|value| value + 1)}>
                    {local.get()}
                </button>
            }
            .render(cx)
        }
    }
}

mod shell {
    use super::*;
    use crate::counter::IndependentCounter;
    use proa_core::WebRenderSync as RenderSync;

    pub struct BoundaryShell {
        pub initial: u32,
    }

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for BoundaryShell {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <section>
                    {rsjs::island(IndependentCounter {
                        initial: self.initial,
                    })}
                </section>
            }
            .render(cx)
        }
    }
}

mod page {
    use super::*;
    use crate::shell::BoundaryShell as ImportedShell;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LivePage;

    #[rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for LivePage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let source = rsjs::signal(7_u32);
            let increment =
                rsjs::event_handler(|_: rsjs::Event| source.update(|value| value + 1));

            proa_macros::html_sync! {
                <main>
                    {ImportedShell {
                        initial: source.get(),
                    }}
                    <button type="button" on_click={increment()}>
                        "Change live source"
                    </button>
                </main>
            }
            .render(cx)
        }
    }
}

fn main() {}
"#;

const STATIC_SLOT_FIXTURE: &str = r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};
use rsjs::rsjs;

mod disclosure {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct Disclosure<C = ()> {
        pub children: C,
    }

    #[rsjs(component, client)]
    impl<L, B, C> RenderSync<L, B> for Disclosure<C>
    where
        B: WriteBuf,
        L: DataLoader,
        C: RenderSync<L, B>,
    {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let open = rsjs::signal(false);
            let toggle =
                rsjs::event_handler(|_: rsjs::Event| open.update(|value| !value));

            proa_macros::html_sync! {
                <section data-disclosure data-open={open.get()}>
                    <button type="button" on_click={toggle()}>
                        "Toggle"
                    </button>
                    <div data-disclosure-content>
                        {self.children}
                    </div>
                </section>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::Disclosure as ImportedDisclosure;
    }
}

mod page {
    use super::*;
    use crate::disclosure::public::ImportedDisclosure;
    use proa_core::WebRenderSync as RenderSync;

    pub struct StaticSlotPage;

    #[rsjs(component)]
    impl<L: DataLoader, B: WriteBuf> RenderSync<L, B> for StaticSlotPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <main data-static-slot-page>
                    {rsjs::island(ImportedDisclosure {
                        children: proa_macros::html_sync! {
                            <article data-static-slot>
                                <h2>"Immutable server article"</h2>
                                <p>"Rendered exactly once."</p>
                            </article>
                        },
                    })}
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
    let html = proa_core::render_to_string(page::StaticSlotPage);
    assert_eq!(
        html.matches("<rsjs-island").count(),
        1,
        "the static page must not become an owner, while the disclosure must: {html}"
    );
    assert_eq!(
        html.matches("<article data-static-slot").count(),
        1,
        "the immutable slot must render exactly once: {html}"
    );
    assert_eq!(
        html.matches("Immutable server article").count(),
        1,
        "slot content must not be duplicated into a browser seed: {html}"
    );
    let slot_start = html
        .find("<article data-static-slot")
        .expect("rendered static slot");
    let slot_end = html[slot_start..]
        .find("</article>")
        .map(|offset| slot_start + offset)
        .expect("closed static slot");
    assert!(
        !html[slot_start..slot_end].contains("data-rsjs-"),
        "immutable server DOM must not acquire client bindings: {}",
        &html[slot_start..slot_end]
    );
    assert!(
        html.contains("data-static-slot-page")
            && html.contains("data-disclosure")
            && html.contains("data-rsjs-handler=")
            && html.contains("data-rsjs-attr=")
            && html.contains("\"signals\":[false]"),
        "the independent disclosure must SSR and retain its own client seed: {html}"
    );
    for (offset, _) in html.match_indices("<script") {
        let script = &html[offset..];
        let end = script
            .find("</script>")
            .expect("every emitted script must be closed");
        assert!(
            !script[..end].contains("Immutable server article"),
            "static slot markup must not be serialized into a script: {}",
            &script[..end]
        );
    }

    let manifest = rsjs::manifest::manifest();
    let disclosure_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "Disclosure")
        .expect("disclosure component fragment");
    assert!(
        manifest
            .component_fragments
            .iter()
            .all(|fragment| fragment.struct_name != "StaticSlotPage"),
        "the static parent must not receive a client component fragment"
    );
    assert!(
        disclosure_fragment
            .js_source
            .contains("bind_delegated_events(root, \"click\"")
            && disclosure_fragment.js_source.contains("bind_attr("),
        "the child owner must hydrate its interaction and open-state attribute:\n{}",
        disclosure_fragment.js_source
    );
    for forbidden in [
        "Immutable server article",
        "data-static-slot",
        "innerHTML",
        "insertAdjacentHTML",
        "createElement(",
    ] {
        assert!(
            !disclosure_fragment.js_source.contains(forbidden),
            "the child module must not reconstruct static slot DOM (`{forbidden}`):\n{}",
            disclosure_fragment.js_source
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
    assert_eq!(
        rendered_owners.len(),
        1,
        "exactly one linked owner must activate for the disclosure"
    );
    assert!(
        rendered_owners[0]
            .component_fragment_imports
            .contains(&disclosure_fragment.asset_id),
        "the rendered owner must import the interactive disclosure fragment"
    );

    print!("{html}");
}
"#;

#[test]
fn explicit_island_accepts_static_scalar_but_rejects_the_same_live_edge() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let static_temp = write_fixture("rsjs_explicit_island_static_scalar_fixture", STATIC_FIXTURE);
    let static_output = run_fixture(
        &static_temp,
        "rsjs_explicit_island_static_scalar_fixture",
        "rsjs-explicit-island-static-scalar",
    )
    .expect("a static scalar may cross into an independent island's SSR input");

    assert_static_boundary_plan(&static_output.semantic);

    let executable = static_output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final static-island fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert!(
        html.contains("data-static-shell")
            && html.contains("data-independent-counter")
            && html.contains(">41</button>")
            && html.contains("\"signals\":[41]"),
        "final executable must preserve the server scalar and independent seed: {html}"
    );

    let live_temp = write_fixture("rsjs_explicit_island_live_scalar_fixture", LIVE_FIXTURE);
    let error = run_fixture(
        &live_temp,
        "rsjs_explicit_island_live_scalar_fixture",
        "rsjs-explicit-island-live-scalar",
    )
    .expect_err("a live parent value must not snapshot through an independent island");
    assert!(
        matches!(
            &error,
            LinkedCargoError::SemanticLink(SemanticLinkError::ComponentGraph(
                ComponentGraphLinkError::ForbiddenLiveBoundaryCrossing {
                    field_name,
                    ..
                }
            )) if field_name == "initial"
        ),
        "expected the canonical forbidden-boundary error, got: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("passes live field \"initial\" through an independent island")
            && message.contains("live values cannot cross `rsjs::island(...)`"),
        "the linked diagnostic must explain the owner cut and remediation: {message}"
    );
}

#[test]
fn explicit_island_preserves_one_immutable_static_html_slot() {
    let _build_guard = FIXTURE_BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = write_fixture(
        "rsjs_explicit_island_static_slot_fixture",
        STATIC_SLOT_FIXTURE,
    );
    let output = run_fixture(
        &temp,
        "rsjs_explicit_island_static_slot_fixture",
        "rsjs-explicit-island-static-slot",
    )
    .expect("one immutable exact-one HTML slot may cross into an independent island");

    assert_static_slot_plan(&output.semantic);

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final static-slot fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    let html = String::from_utf8(runtime.stdout).unwrap();
    assert_eq!(html.matches("<article data-static-slot").count(), 1);
    assert_eq!(html.matches("Immutable server article").count(), 1);
    assert!(
        html.contains("data-static-slot-page")
            && html.contains("data-disclosure")
            && html.contains("\"signals\":[false]"),
        "final executable must preserve one server slot and an independently hydrated child: {html}"
    );
}

fn assert_static_boundary_plan(plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan) {
    let shell_key = component_key(
        "rsjs_explicit_island_static_scalar_fixture",
        "StaticShell",
        ["shell"],
    );
    let child_key = component_key(
        "rsjs_explicit_island_static_scalar_fixture",
        "IndependentCounter",
        ["counter"],
    );
    let shell = plan
        .components
        .iter()
        .find(|component| component.semantic_key == shell_key)
        .expect("canonical static shell");
    let DescriptorBody::Component(shell_descriptor) = &shell.descriptor.body else {
        panic!("static shell descriptor must be a component");
    };
    let edge = shell_descriptor
        .direct_components
        .values()
        .next()
        .expect("explicit child edge");
    assert_eq!(edge.boundary, ComponentBoundary::IndependentIsland);
    assert!(matches!(
        edge.bindings.get("initial").map(|binding| &binding.source),
        Some(ComponentBindingSource::ServerOnly)
    ));
    assert!(
        shell_descriptor
            .prop_uses
            .values()
            .any(|prop| prop.temporal == TemporalPropMode::ForbiddenBoundaryCrossing),
        "the shell must retain a latent rejection for a future live caller"
    );

    let shell_site = plan
        .sites
        .iter()
        .find(|site| site.root == shell_key && site.component == shell_key)
        .expect("static shell root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == shell_key && site.component == child_key)
        .expect("independent child site");
    assert_eq!(child_site.parent, Some(shell_site.candidate));
    assert!(
        plan.component_inputs.live_props.iter().all(|prop| {
            (prop.parent, prop.child) != (shell_site.candidate, child_site.candidate)
        }),
        "a static explicit-island input must not become a live prop or snapshot"
    );
    let shell_owner = plan.owners.candidate_to_owner.get(&shell_site.candidate);
    let child_owner = plan.owners.candidate_to_owner.get(&child_site.candidate);
    assert!(
        child_owner.is_some() && shell_owner != child_owner,
        "the interactive child must activate independently of the static shell"
    );
}

fn assert_static_slot_plan(plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan) {
    let page_key = component_key(
        "rsjs_explicit_island_static_slot_fixture",
        "StaticSlotPage",
        ["page"],
    );
    let child_key = component_key(
        "rsjs_explicit_island_static_slot_fixture",
        "Disclosure",
        ["disclosure"],
    );
    let page = plan
        .components
        .iter()
        .find(|component| component.semantic_key == page_key)
        .expect("canonical static-slot page");
    let DescriptorBody::Component(page_descriptor) = &page.descriptor.body else {
        panic!("static-slot page descriptor must be a component");
    };
    let edge = page_descriptor
        .direct_components
        .values()
        .next()
        .expect("explicit disclosure edge");
    assert_eq!(page_descriptor.direct_components.len(), 1);
    assert_eq!(edge.boundary, ComponentBoundary::IndependentIsland);
    assert_eq!(edge.bindings.len(), 1);
    assert!(matches!(
        edge.bindings.get("children").map(|binding| &binding.source),
        Some(ComponentBindingSource::StaticSlot {
            span: SpanCardinality::Exact(1)
        })
    ));

    let child = plan
        .components
        .iter()
        .find(|component| component.semantic_key == child_key)
        .expect("canonical disclosure");
    let DescriptorBody::Component(child_descriptor) = &child.descriptor.body else {
        panic!("disclosure descriptor must be a component");
    };
    assert!(
        !child_descriptor.root_eligible,
        "a component whose SSR requires an incoming static slot must not gain a standalone root"
    );
    assert!(
        plan.sites
            .iter()
            .all(|site| site.component != child_key || site.parent.is_some()),
        "the disclosure must exist only at its validated incoming component edge"
    );
    let slot = child_descriptor
        .fields
        .values()
        .find(|field| field.source_name == "children")
        .expect("disclosure children field");
    assert_eq!(child_descriptor.fields.len(), 1);
    assert_eq!(slot.role, ComponentFieldRole::Slot);
    assert_eq!(slot.type_witness, None);
    assert_eq!(slot.crossing_hook, None);
    assert_eq!(slot.client_value, None);
    assert_eq!(slot.callback_shape, None);
    assert_eq!(slot.codec, ComponentFieldCodec::None);
    assert_eq!(
        slot.static_slot,
        StaticSlotCapability::Immutable {
            permits_nested_owner: false,
        }
    );

    let page_site = plan
        .sites
        .iter()
        .find(|site| site.root == page_key && site.component == page_key)
        .expect("static-slot page root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == page_key && site.component == child_key)
        .expect("independent disclosure site");
    assert_eq!(child_site.parent, Some(page_site.candidate));
    assert!(
        !plan
            .owners
            .candidate_to_owner
            .contains_key(&page_site.candidate),
        "the static page has no browser obligation"
    );
    assert!(
        plan.owners
            .candidate_to_owner
            .contains_key(&child_site.candidate),
        "the disclosure must hydrate independently"
    );
    assert!(
        plan.component_inputs.live_props.is_empty()
            && plan.component_inputs.live_callbacks.is_empty(),
        "an immutable static slot must not create a live edge or serialized callback"
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
