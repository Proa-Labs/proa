// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end acceptance coverage for plan-specialized component render sites.
//!
//! This deliberately exercises the final Rust compilation and the resulting
//! executable. Link-plan structure alone cannot prove that the proc macro kept
//! the original Rust expressions or delegated the authored sync/async render
//! methods without changing their semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;

use proa_cli::rsjs::build::DescriptorCargoRequest;
use proa_cli::rsjs::pipeline::{run_linked_cargo, DescriptorRootSelector, LinkedCargoRequest};
use rsjs_ir::abi::{
    AbiVersion, ComponentSemanticKey, InvocationId, ProducerVersion, TemporalPropMode,
};
use rsjs_ir::build::ArtifactRole;
use tempfile::TempDir;

#[test]
fn final_codegen_selects_distinct_imported_child_sites_for_sync_and_async_rendering() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    let proa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"rsjs_linked_site_codegen_fixture\"\n\
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
    fs::write(
        temp.path().join("src/main.rs"),
        r#"
#![allow(dead_code, unexpected_cfgs, unused_imports)]

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use proa_core::{
    DataLoader, RenderOutcome, WebContext, WriteBuf, WriteError,
};
use rsjs::rsjs;

mod sync_ui {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct StatusBadge {
        pub label: &'static str,
    }

    #[rsjs(component)]
    impl<B: WriteBuf> RenderSync<::proa_core::NoLoader, B> for StatusBadge {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-sync-badge>{self.label}</span>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::StatusBadge as PublicStatusBadge;
    }
}

mod async_ui {
    use super::*;
    use proa_core::WebRender as RenderAsync;

    pub struct AsyncStatusBadge {
        pub label: &'static str,
    }

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for AsyncStatusBadge {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                proa_macros::html! {
                    <em data-async-badge>{self.label}</em>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }

    pub mod public {
        pub use super::AsyncStatusBadge as PublicAsyncStatusBadge;
    }
}

mod sync_page {
    use super::*;
    use crate::sync_ui::public::PublicStatusBadge as ImportedBadge;
    use proa_core::WebRenderSync as RenderSync;

    pub struct SyncPage;

    #[rsjs(component)]
    impl<B: WriteBuf> RenderSync<::proa_core::NoLoader, B> for SyncPage {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            let first = "sync-one";
            let second = "sync-two";
            proa_macros::html_sync! {
                <main>
                    {ImportedBadge { label: first }}
                    {ImportedBadge { label: second }}
                </main>
            }
            .render(cx)
        }
    }
}

mod async_page {
    use super::*;
    use crate::async_ui::public::PublicAsyncStatusBadge as ImportedAsyncBadge;
    use proa_core::WebRender as RenderAsync;

    pub struct AsyncPage;

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for AsyncPage {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                let first = "async-one";
                let second = "async-two";
                proa_macros::html! {
                    <section>
                        {ImportedAsyncBadge { label: first }}
                        {ImportedAsyncBadge { label: second }}
                    </section>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

mod unit_ui {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct UnitBadge;

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for UnitBadge {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <i data-unit-badge>"unit"</i>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::UnitBadge as PublicUnitBadge;
    }
}

mod unit_page {
    use super::*;
    use crate::unit_ui::public::PublicUnitBadge as RenamedUnitBadge;
    use proa_core::WebRenderSync as RenderSync;

    pub struct UnitPage;

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for UnitPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let rendered_value = "local-value";
            proa_macros::html_sync! {
                <aside>
                    {RenamedUnitBadge}
                    {crate::unit_ui::UnitBadge}
                    {rendered_value}
                </aside>
            }
            .render(cx)
        }
    }
}

mod live_ui {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LiveButton {
        pub value: String,
        pub on_increment: rsjs::Handler<String>,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for LiveButton {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let increment = rsjs::event_handler(|_: rsjs::Event| {
                self.on_increment.call(self.value);
            });
            proa_macros::html_sync! {
                <button
                    type="button"
                    data-live-child
                    on_click={increment()}
                >
                    {self.value}
                </button>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::LiveButton as PublicLiveButton;
    }
}

mod live_label {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LiveLabel {
        pub value: String,
        pub on_increment: rsjs::Handler<String>,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for LiveLabel {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let increment = rsjs::event_handler(|_: rsjs::Event| {
                self.on_increment.call(self.value);
            });
            proa_macros::html_sync! {
                <button type="button" data-live-label on_click={increment()}>
                    "before:"
                    {self.value}
                    ":after"
                </button>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::LiveLabel as PublicLiveLabel;
    }
}

mod live_presenter {
    use super::*;
    use crate::live_label::public::PublicLiveLabel as ImportedLiveLabel;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LivePresenter {
        pub value: String,
        pub on_increment: rsjs::Handler<String>,
    }

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for LivePresenter {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <aside data-live-presenter>
                    {ImportedLiveLabel {
                        value: self.value,
                        on_increment: self.on_increment,
                    }}
                </aside>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::LivePresenter as PublicLivePresenter;
    }
}

mod seeded_ui {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct SeededCounter {
        pub initial: u32,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for SeededCounter {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let local = rsjs::signal::<u32>(self.initial);

            proa_macros::html_sync! {
                <button
                    type="button"
                    data-seeded-counter
                    on_click={|_| local.update(|value| value + 1)}
                >
                    {local.get()}
                </button>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::SeededCounter as PublicSeededCounter;
    }
}

mod conditional_label {
    use super::*;
    use proa_core::WebRenderSync as RenderSync;

    pub struct ConditionalLabel {
        pub value: u32,
    }

    #[rsjs(component)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for ConditionalLabel {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <strong data-conditional-label>{self.value}</strong>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::ConditionalLabel as PublicConditionalLabel;
    }
}

mod conditional_ui {
    use super::*;
    use crate::conditional_label::public::PublicConditionalLabel as ImportedConditionalLabel;
    use proa_core::WebRenderSync as RenderSync;

    pub struct ConditionalChild {
        pub initial: u32,
        pub value: u32,
        pub on_change: rsjs::Handler<u32>,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for ConditionalChild {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let local = rsjs::signal::<u32>(self.initial);
            let change = rsjs::event_handler(|_: rsjs::Event| {
                self.on_change.call(self.value);
            });

            proa_macros::html_sync! {
                <article data-conditional-child>
                    {ImportedConditionalLabel { value: self.value }}
                    <button type="button" data-conditional-change on_click={change()}>
                        "Change"
                    </button>
                    <output data-conditional-local>{local.get()}</output>
                </article>
            }
            .render(cx)
        }
    }

    pub mod public {
        pub use super::ConditionalChild as PublicConditionalChild;
    }
}

mod conditional_page {
    use super::*;
    use crate::conditional_ui::public::PublicConditionalChild as ImportedConditionalChild;
    use proa_core::WebRenderSync as RenderSync;

    pub struct ConditionalPage;

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for ConditionalPage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let visible = rsjs::signal(false);
            let value = rsjs::signal(7_u32);
            let on_change = rsjs::handler(|next: u32| value.set(next));

            proa_macros::html_sync! {
                <section data-conditional-page>
                    {if visible.get() {
                        proa_macros::html_sync! {
                            <div data-conditional-host>
                                {ImportedConditionalChild {
                                    initial: value.get(),
                                    value: value.get(),
                                    on_change,
                                }}
                            </div>
                        }
                    }}
                    <button
                        type="button"
                        data-toggle-conditional-child
                        on_click={|_| visible.set(!visible.get())}
                    >
                        "Toggle"
                    </button>
                </section>
            }
            .render(cx)
        }
    }
}

mod query_ui {
    use super::*;
    use proa_core::WebRender as RenderAsync;

    pub struct QueryResults {
        pub topic: &'static str,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for QueryResults {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                let results: rsjs::Query<String> = rsjs::query_with_initial(
                    rsjs::QueryKey::new(("results", self.topic)),
                    String::from("ready"),
                    rsjs::FetchRequest {
                        url: "/api/results",
                        response: Some(rsjs::ResponseKind::Text),
                        ..Default::default()
                    },
                    rsjs::QueryOptions::default(),
                );

                proa_macros::html! {
                    <section data-query-results>
                        <output>{results.data()}</output>
                        <button type="button" on_click={|_| results.refetch()}>
                            "Refresh"
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
        pub use super::QueryResults as PublicQueryResults;
    }
}

mod query_page {
    use super::*;
    use crate::query_ui::public::PublicQueryResults as ImportedQueryResults;
    use proa_core::WebRender as RenderAsync;

    pub struct QueryPage;

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for QueryPage {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                let topic = rsjs::signal("rust");

                proa_macros::html! {
                    <main data-query-page>
                        {ImportedQueryResults {
                            topic: topic.get(),
                        }}
                        <button
                            type="button"
                            data-change-query-key
                            on_click={|_| topic.set("web")}
                        >
                            "Change topic"
                        </button>
                    </main>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

mod mutation_ui {
    use super::*;
    use proa_core::WebRender as RenderAsync;

    pub struct MutationResults {
        pub label: &'static str,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for MutationResults {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                let save = rsjs::mutation_with_options::<&'static str, ()>(
                    rsjs::QueryKey::new(("save-results", "rust")),
                    rsjs::FetchRequest {
                        url: "/api/save-results",
                        method: Some(rsjs::HttpMethod::Patch),
                        credentials: Some(rsjs::FetchCredentials::SameOrigin),
                        response: Some(rsjs::ResponseKind::Empty),
                        headers: &[("X-Mutation", "linked-owner")],
                        body: None,
                        timeout_ms: Some(2_500),
                    },
                    &[rsjs::QueryKey::new(("results", "rust"))],
                    rsjs::MutationOptions {
                        retry_count: 2,
                        on_success_invalidate_strategy: rsjs::InvalidationStrategy::Reset,
                    },
                );

                proa_macros::html! {
                    <section data-mutation-results>
                        <output>{save.status()}</output>
                        <span>{self.label}</span>
                        <button
                            type="button"
                            data-run-mutation
                            on_click={|_| save.run(self.label)}
                        >
                            "Save"
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
        pub use super::MutationResults as PublicMutationResults;
    }
}

mod mutation_page {
    use super::*;
    use crate::mutation_ui::public::PublicMutationResults as ImportedMutationResults;
    use proa_core::WebRender as RenderAsync;

    pub struct MutationPage;

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderAsync<L, B> for MutationPage {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
        {
            RenderOutcome::pending(async move {
                let label = rsjs::signal("ready");

                proa_macros::html! {
                    <main data-mutation-page>
                        {ImportedMutationResults {
                            label: label.get(),
                        }}
                        <button
                            type="button"
                            data-change-mutation-label
                            on_click={|_| label.set("changed")}
                        >
                            "Change label"
                        </button>
                    </main>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

mod live_page {
    use super::*;
    use crate::live_presenter::public::PublicLivePresenter as ImportedLivePresenter;
    use crate::live_ui::public::PublicLiveButton as ImportedLiveButton;
    use crate::seeded_ui::public::PublicSeededCounter as ImportedSeededCounter;
    use proa_core::WebRenderSync as RenderSync;

    pub struct LivePage;

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: DataLoader> RenderSync<L, B> for LivePage {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let count = rsjs::signal(String::from("seven"));
            let seed_source = rsjs::signal(7_u32);
            let on_increment = rsjs::handler(|next: String| count.set(next));
            let focus_target = rsjs::node_ref::<rsjs::HtmlInputElement>();

            proa_macros::html_sync! {
                <section data-live-page>
                    <input data-live-ref ref={focus_target} />
                    <button
                        type="button"
                        data-focus-live-ref
                        on_click={|_| focus_target.focus()}
                    >
                        "Focus input"
                    </button>
                    {ImportedLiveButton {
                        value: count.get(),
                        on_increment,
                    }}
                    {ImportedLivePresenter {
                        value: count.get(),
                        on_increment,
                    }}
                    {ImportedSeededCounter {
                        initial: seed_source.get(),
                    }}
                    <button
                        type="button"
                        data-change-seed-source
                        on_click={|_| seed_source.update(|value| value + 1)}
                    >
                        "Change parent seed"
                    </button>
                    <output>{count.get()}</output>
                </section>
            }
            .render(cx)
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
    let sync = proa_core::render_to_string(sync_page::SyncPage);
    let asynchronous = block_on(proa_core::render_to_string_async(
        async_page::AsyncPage,
    ));
    let query = block_on(proa_core::render_to_string_async(
        query_page::QueryPage,
    ));
    let mutation = block_on(proa_core::render_to_string_async(
        mutation_page::MutationPage,
    ));
    let conditional = proa_core::render_to_string(
        conditional_page::ConditionalPage,
    );
    let unit = proa_core::render_to_string(unit_page::UnitPage);
    let live = proa_core::render_to_string(live_page::LivePage);
    let manifest = rsjs::manifest::manifest();

    assert_eq!(unit.matches("<i data-unit-badge").count(), 2, "{unit}");
    assert!(
        unit.ends_with("</i>local-value</aside>"),
        "a prelude local path must remain an ordinary renderable after unit component edges: {unit}"
    );
    assert!(
        manifest
            .islands
            .iter()
            .all(|island| island.struct_name.starts_with("LinkedOwner")),
        "only linked-owner modules, never raw component fragments, may register as islands"
    );
    let fused_owners = manifest
        .islands
        .iter()
        .filter(|island| island.component_fragment_imports.len() == 5)
        .collect::<Vec<_>>();
    assert_eq!(
        fused_owners.len(),
        1,
        "the live parent/child graph must produce exactly one fused owner"
    );
    let owner = *fused_owners[0];
    assert!(
        owner.struct_name.starts_with("LinkedOwner"),
        "the manifest entry must be the linked owner, not a component fragment"
    );
    assert_eq!(
        owner.component_fragment_imports.len(),
        5,
        "the owner imports the parent, interactive child, presenter, leaf, and seeded fragments"
    );
    let query_owners = manifest
        .islands
        .iter()
        .filter(|island| island.uses_queries && island.component_fragment_imports.len() == 2)
        .collect::<Vec<_>>();
    assert_eq!(
        query_owners.len(),
        1,
        "the live query-key parent and query child must form one query-enabled owner"
    );
    let query_owner = *query_owners[0];
    let mutation_owners = manifest
        .islands
        .iter()
        .filter(|island| island.uses_mutations && island.component_fragment_imports.len() == 2)
        .collect::<Vec<_>>();
    assert_eq!(
        mutation_owners.len(),
        1,
        "the live mutation parent and imported mutation child must form one mutation-enabled owner"
    );
    let mutation_owner = *mutation_owners[0];
    let conditional_owners = manifest
        .islands
        .iter()
        .filter(|island| {
            !island.uses_queries
                && !island.uses_mutations
                && island.component_fragment_imports.len() == 3
        })
        .collect::<Vec<_>>();
    assert_eq!(
        conditional_owners.len(),
        1,
        "the conditional parent, child, and static descendant must produce exactly one owner"
    );
    let conditional_owner = *conditional_owners[0];
    assert_eq!(
        manifest.component_fragments.len(),
        12,
        "all twelve linked component definitions remain reusable fragments"
    );

    let parent_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "LivePage")
        .expect("LivePage fragment");
    let child_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "LiveButton")
        .expect("LiveButton fragment");
    let presenter_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "LivePresenter")
        .expect("LivePresenter fragment");
    let label_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "LiveLabel")
        .expect("LiveLabel fragment");
    let seeded_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "SeededCounter")
        .expect("SeededCounter fragment");
    let query_page_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "QueryPage")
        .expect("QueryPage fragment");
    let query_results_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "QueryResults")
        .expect("QueryResults fragment");
    let mutation_page_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "MutationPage")
        .expect("MutationPage fragment");
    let mutation_results_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "MutationResults")
        .expect("MutationResults fragment");
    let conditional_page_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "ConditionalPage")
        .expect("ConditionalPage fragment");
    let conditional_child_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "ConditionalChild")
        .expect("ConditionalChild fragment");
    let conditional_label_fragment = manifest
        .component_fragments
        .iter()
        .find(|fragment| fragment.struct_name == "ConditionalLabel")
        .expect("ConditionalLabel fragment");
    for fragment in [
        parent_fragment,
        child_fragment,
        presenter_fragment,
        label_fragment,
        seeded_fragment,
    ] {
        assert!(
            owner.component_fragment_imports.contains(&fragment.asset_id),
            "linked owner must import {}",
            fragment.asset_id
        );
    }

    assert_eq!(live.matches("<rsjs-island").count(), 5, "{live}");
    assert!(
        live.contains("<button type=\"button\" data-live-label")
            && live.contains("before:<span data-rsjs-bind=\"")
            && live.contains(">seven</span>:after</button>"),
        "the owned mixed-content LiveLabel occurrence must retain a carrier and its surrounding static text: {live}"
    );
    let live_button_attr = live.find("data-live-child").expect("live child button");
    let live_button_open_start = live[..live_button_attr]
        .rfind('<')
        .expect("live child opening tag");
    let live_button_open_end = live[live_button_attr..]
        .find('>')
        .map(|offset| live_button_attr + offset)
        .expect("live child opening tag terminator");
    let live_button_open = &live[live_button_open_start..=live_button_open_end];
    assert!(
        live_button_open.contains("data-rsjs-bind=\""),
        "the existing single-child element binding must retain its direct marker: {live_button_open}"
    );
    assert_eq!(
        live.matches("data-rsjs-linked-candidate=").count(),
        5,
        "{live}"
    );
    assert_eq!(
        live.matches(&format!("data-rsjs-island-root=\"{}\"", owner.asset_id))
            .count(),
        1,
        "only the owner root uses the manifest asset identity: {live}"
    );
    assert_eq!(
        live.matches("<script type=\"application/json\" data-rsjs-island=")
            .count(),
        5,
        "each fused fragment retains a deterministic seed boundary: {live}"
    );

    let owner_js = owner.js_source;
    assert!(
        owner_js.contains("__rsjs_expose_0?.signals?.[")
            && owner_js.contains("const __rsjs_signal_1_")
            && owner_js.contains("__rsjs_signals_1["),
        "owner module must alias the parent's live signal into the child:\n{owner_js}"
    );
    assert!(
        owner_js.contains("__rsjs_expose_0?.payloadHandlers?.[")
            && owner_js.contains("__rsjs_callbacks_1[\"__callback_")
            && owner_js.contains("(__payload) => __rsjs_payload_handler_1_"),
        "owner module must resolve the current parent payload handler for the child:\n{owner_js}"
    );
    assert!(
        parent_fragment
            .js_source
            .contains("options.expose.signals[")
            && parent_fragment
                .js_source
                .contains("options.expose.payloadHandlers["),
        "the parent fragment must expose both linked endpoints:\n{}",
        parent_fragment.js_source
    );
    assert!(
        parent_fragment
            .js_source
            .contains("const __ref_0 = { current: null };")
            && parent_fragment
                .js_source
                .contains("__rsjs_disposers.push(() => { __ref_0.current = null; });")
            && parent_fragment
                .js_source
                .contains("__ref_0.current?.focus?.();")
            && parent_fragment.js_source.contains("\"data-rsjs-ref\""),
        "the linked root fragment must preserve node-ref declaration, binding, use, and cleanup:\n{}",
        parent_fragment.js_source
    );
    assert!(
        child_fragment
            .js_source
            .contains("const __rsjs_signal_overrides = options.signals ?? {};")
            && child_fragment
                .js_source
                .contains("root.__rsjs_callbacks?.__callback_"),
        "the child must consume the aliased signal and look up the callback at invocation time:\n{}",
        child_fragment.js_source
    );
    assert!(
        owner_js.matches("?.signals?.[").count() >= 3,
        "the owner must carry both direct live aliases plus the presentational pass-through:\n{owner_js}"
    );
    assert!(
        presenter_fragment
            .js_source
            .contains("options.expose.signals[")
            && label_fragment
                .js_source
                .contains("const __rsjs_signal_overrides = options.signals ?? {};")
            && label_fragment
                .js_source
                .contains("root.__rsjs_callbacks?.__callback_"),
        "the presenter must forward its inbound value and callback to the terminal leaf"
    );
    assert!(
        owner_js
            .matches("__rsjs_expose_0?.payloadHandlers?.[")
            .count()
            >= 2
            && owner_js.contains("__rsjs_callbacks_3[\"__callback_")
            && !owner_js.contains("__rsjs_expose_2?.payloadHandlers?.["),
        "callback forwarding must flatten root → presenter → leaf without inventing a presenter payload handler:\n{owner_js}"
    );
    assert!(
        seeded_fragment
            .js_source
            .contains("const __rsjs_signal_seeds = options.signalSeeds ?? {};")
            && seeded_fragment.js_source.contains(
                "__rsjs_signal_overrides[0] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0) ? integer_signal(__rsjs_signal_seeds[0], \"u32\")"
            ),
        "the SeedOnce child must retain a child-owned local signal rather than requiring a parent alias:\n{}",
        seeded_fragment.js_source
    );
    for fragment in [query_page_fragment, query_results_fragment] {
        assert!(
            query_owner
                .component_fragment_imports
                .contains(&fragment.asset_id),
            "query owner must import {}",
            fragment.asset_id
        );
    }
    assert!(
        query_results_fragment
            .js_source
            .contains("root: options.seedRoot ?? root,")
            && query_results_fragment
                .js_source
                .contains("rebind_query("),
        "the query fragment must read adjacent SSR seeds from the wrapper and rebind its live key:\n{}",
        query_results_fragment.js_source
    );
    assert_eq!(
        query.matches("data-rsjs-linked-candidate=").count(),
        2,
        "{query}"
    );
    assert!(
        query.contains("data-rsjs-island-queries="),
        "the committed query occurrence must retain its adjacent query seed: {query}"
    );
    for fragment in [mutation_page_fragment, mutation_results_fragment] {
        assert!(
            mutation_owner
                .component_fragment_imports
                .contains(&fragment.asset_id),
            "mutation owner must import {}",
            fragment.asset_id
        );
    }
    let mutation_js = mutation_results_fragment.js_source;
    assert!(
        mutation_js.contains("root: options.seedRoot ?? root,")
            && mutation_js.contains("register_mutation(")
            && mutation_js.contains("\\\"url\\\":\\\"/api/save-results\\\"")
            && mutation_js.contains("\\\"method\\\":\\\"PATCH\\\"")
            && mutation_js.contains("\\\"credentials\\\":\\\"same-origin\\\"")
            && mutation_js.contains("\\\"X-Mutation\\\",\\\"linked-owner\\\"")
            && mutation_js.contains("\\\"timeout_ms\\\":2500")
            && mutation_js.contains("invalidates: [[\"results\", \"rust\"]]")
            && mutation_js.contains("retry_count: 2")
            && mutation_js.contains("on_success_invalidate_strategy: \"reset\""),
        "the mutation fragment must preserve the exact request, invalidation, options, and wrapper seed root:\n{mutation_js}"
    );
    assert!(
        mutation_js.contains("__rsjs_disposers.push("),
        "the linked mutation fragment must retain cleanup:\n{mutation_js}"
    );
    assert_eq!(
        mutation.matches("data-rsjs-linked-candidate=").count(),
        2,
        "{mutation}"
    );
    assert!(
        mutation.contains("data-rsjs-island-mutations="),
        "the committed mutation occurrence must retain its adjacent mutation metadata: {mutation}"
    );
    for fragment in [
        conditional_page_fragment,
        conditional_child_fragment,
        conditional_label_fragment,
    ] {
        assert!(
            conditional_owner
                .component_fragment_imports
                .contains(&fragment.asset_id),
            "conditional owner must import {}",
            fragment.asset_id
        );
    }
    assert_eq!(
        conditional.matches("data-rsjs-linked-candidate=").count(),
        3,
        "the retained conditional subtree must keep one wrapper per linked candidate: {conditional}"
    );
    assert_eq!(
        conditional
            .matches(&format!(
                "data-rsjs-island-root=\"{}\"",
                conditional_owner.asset_id
            ))
            .count(),
        1,
        "the conditional graph must register exactly one linked owner asset: {conditional}"
    );
    assert!(
        conditional.contains("data-conditional-child")
            && conditional.contains("data-conditional-label")
            && conditional.contains("data-rsjs-cond-then=")
            && conditional.contains("display:none"),
        "the initially inactive conditional branch must remain in retained SSR with its branch marker: {conditional}"
    );
    let conditional_owner_js = conditional_owner.js_source;
    assert!(
        conditional_owner_js.contains("bind_component_lifecycle")
            && conditional_owner_js.contains("signalSeeds")
            && conditional_owner_js.contains("lifecycleControls"),
        "the final owner must bind conditional mount/dispose and seed each fresh child mount:\n{conditional_owner_js}"
    );
    assert!(
        conditional_page_fragment
            .js_source
            .contains("options.expose.lifecycleControls["),
        "the parent fragment must export its exact conditional predicate:\n{}",
        conditional_page_fragment.js_source
    );
    assert!(
        conditional_child_fragment
            .js_source
            .contains("const __rsjs_signal_seeds = options.signalSeeds ?? {};")
            && conditional_child_fragment
                .js_source
                .contains("Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0)"),
        "the child fragment must reconstruct SeedOnce state from a fresh lifecycle seed:\n{}",
        conditional_child_fragment.js_source
    );
    assert!(
        conditional_label_fragment
            .js_source
            .contains("const __rsjs_signal_overrides = options.signals ?? {};"),
        "the static descendant must consume its live alias only while its conditional group is mounted:\n{}",
        conditional_label_fragment.js_source
    );

    print!("{sync}|{asynchronous}");
}
"#,
    )
    .unwrap();

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let registry_environment = BTreeMap::from([(
        "CARGO_REGISTRIES_PROA_INDEX".into(),
        "sparse+https://registry.proa.so/sparse/".into(),
    )]);
    let metadata = Command::new(&cargo)
        .current_dir(temp.path())
        .envs(&registry_environment)
        .args(["metadata", "--format-version=1", "--no-deps", "--offline"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package_id = metadata["packages"][0]["id"].as_str().unwrap().to_string();

    let output = run_linked_cargo(&LinkedCargoRequest {
        descriptor: DescriptorCargoRequest {
            cargo,
            wrapper: std::path::PathBuf::from(env!("CARGO_BIN_EXE_cargo-proa")),
            working_directory: temp.path().to_path_buf(),
            staging_directory: temp.path().join("proa-generated/descriptor-staging"),
            target_directory: temp.path().join("proa-generated/descriptor-target"),
            source_exclusion_directory: temp.path().join("proa-generated"),
            cargo_args: vec!["build".into(), "--offline".into()],
            environment: registry_environment.clone(),
        },
        root: DescriptorRootSelector {
            package_id,
            target_name: "rsjs_linked_site_codegen_fixture".to_string(),
            target_kinds: BTreeSet::from(["bin".to_string()]),
            role: ArtifactRole::RootExecutable,
            artifact_path: None,
        },
        invocation: InvocationId::derive("rsjs-linked-site-codegen-fixture"),
        producer: ProducerVersion::new(0, 2, 0),
        runtime_protocol: AbiVersion::new(1, 0),
        plan_directory: temp.path().join("proa-generated/plans"),
        final_target_root: temp.path().join("proa-generated/final-target"),
        final_cargo_args: vec!["build".into(), "--offline".into()],
        final_environment: registry_environment,
    })
    .unwrap();

    assert_eq!(output.transport.components.len(), 18);
    assert_eq!(output.semantic.components.len(), 18);
    assert_two_distinct_child_sites(
        &output.semantic,
        component_key("SyncPage", ["sync_page"]),
        component_key("StatusBadge", ["sync_ui"]),
    );
    assert_two_distinct_child_sites(
        &output.semantic,
        component_key("AsyncPage", ["async_page"]),
        component_key("AsyncStatusBadge", ["async_ui"]),
    );
    assert_two_distinct_child_sites(
        &output.semantic,
        component_key("UnitPage", ["unit_page"]),
        component_key("UnitBadge", ["unit_ui"]),
    );
    assert_one_fused_live_owner(
        &output.semantic,
        component_key("LivePage", ["live_page"]),
        component_key("LiveButton", ["live_ui"]),
    );
    assert_live_prop_pass_through(
        &output.semantic,
        component_key("LivePage", ["live_page"]),
        component_key("LivePresenter", ["live_presenter"]),
        component_key("LiveLabel", ["live_label"]),
    );
    assert_seed_once_local_state(
        &output.semantic,
        component_key("LivePage", ["live_page"]),
        component_key("SeededCounter", ["seeded_ui"]),
    );
    assert_live_query_key(
        &output.semantic,
        component_key("QueryPage", ["query_page"]),
        component_key("QueryResults", ["query_ui"]),
    );
    assert_linked_mutation_owner(
        &output.semantic,
        component_key("MutationPage", ["mutation_page"]),
        component_key("MutationResults", ["mutation_ui"]),
    );
    assert_conditional_component_lifecycle(
        &output.semantic,
        component_key("ConditionalPage", ["conditional_page"]),
        component_key("ConditionalChild", ["conditional_ui"]),
        component_key("ConditionalLabel", ["conditional_label"]),
    );

    let executable = output
        .final_root_artifact
        .executable
        .as_ref()
        .expect("final linked-site fixture executable");
    let runtime = Command::new(executable).output().unwrap();
    assert!(
        runtime.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime.stderr)
    );
    assert_eq!(
        String::from_utf8(runtime.stdout).unwrap(),
        "<main><span data-sync-badge>sync-one</span>\
         <span data-sync-badge>sync-two</span></main>|\
         <section><em data-async-badge>async-one</em>\
         <em data-async-badge>async-two</em></section>"
    );
}

fn component_key<I>(name: &str, module_path: I) -> ComponentSemanticKey
where
    I: IntoIterator,
    I::Item: Into<String>,
{
    ComponentSemanticKey::v1(
        "rsjs_linked_site_codegen_fixture",
        "rsjs_linked_site_codegen_fixture",
        module_path,
        name,
    )
    .unwrap()
}

fn assert_two_distinct_child_sites(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
) {
    let sites = plan
        .sites
        .iter()
        .filter(|site| site.root == root && site.component == child)
        .collect::<Vec<_>>();
    assert_eq!(sites.len(), 2, "expected two concrete child occurrences");
    assert_ne!(sites[0].candidate, sites[1].candidate);
    assert_ne!(sites[0].incoming_edge, sites[1].incoming_edge);
    assert_eq!(sites[0].structural_path.len(), 1);
    assert_eq!(sites[1].structural_path.len(), 1);
}

fn assert_one_fused_live_owner(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("live root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == child)
        .expect("live child site");

    assert_eq!(child_site.parent, Some(root_site.candidate));
    let live_props = plan
        .component_inputs
        .live_props
        .iter()
        .filter(|prop| (prop.parent, prop.child) == (root_site.candidate, child_site.candidate))
        .collect::<Vec<_>>();
    assert_eq!(
        live_props.len(),
        2,
        "the child reads its live String prop in both the handler and text binding"
    );
    let live_callbacks = plan
        .component_inputs
        .live_callbacks
        .iter()
        .filter(|callback| {
            (callback.parent, callback.child) == (root_site.candidate, child_site.candidate)
        })
        .collect::<Vec<_>>();
    assert_eq!(live_callbacks.len(), 1);
    let live_callback = live_callbacks[0];
    assert_eq!(
        (live_callback.parent, live_callback.child),
        (root_site.candidate, child_site.candidate)
    );
    assert_eq!(
        plan.owners.candidate_to_owner[&root_site.candidate],
        plan.owners.candidate_to_owner[&child_site.candidate],
        "a live value and callback edge must fuse parent and child"
    );
}

fn assert_live_prop_pass_through(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    presenter: ComponentSemanticKey,
    leaf: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("pass-through root site");
    let presenter_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == presenter)
        .expect("presenter site");
    let leaf_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == leaf)
        .expect("presentational leaf site");

    assert_eq!(presenter_site.parent, Some(root_site.candidate));
    assert_eq!(leaf_site.parent, Some(presenter_site.candidate));
    for (parent, child, label) in [
        (
            root_site.candidate,
            presenter_site.candidate,
            "root-to-presenter live prop",
        ),
        (
            presenter_site.candidate,
            leaf_site.candidate,
            "presenter-to-leaf pass-through",
        ),
    ] {
        assert!(
            plan.component_inputs
                .live_props
                .iter()
                .any(|prop| (prop.parent, prop.child) == (parent, child)),
            "missing {label}"
        );
        assert_eq!(
            plan.owners.candidate_to_owner[&parent], plan.owners.candidate_to_owner[&child],
            "{label} must remain inside one fused owner"
        );
    }
    for (parent, child, label) in [
        (
            root_site.candidate,
            presenter_site.candidate,
            "root-to-presenter callback",
        ),
        (
            presenter_site.candidate,
            leaf_site.candidate,
            "presenter-to-leaf callback forwarding",
        ),
    ] {
        assert!(
            plan.component_inputs
                .live_callbacks
                .iter()
                .any(|callback| (callback.parent, callback.child) == (parent, child)),
            "missing {label}"
        );
    }
}

fn assert_seed_once_local_state(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("SeedOnce root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == child)
        .expect("SeedOnce child site");
    assert_eq!(child_site.parent, Some(root_site.candidate));

    let seed_uses = plan
        .component_inputs
        .live_props
        .iter()
        .filter(|prop| {
            (prop.parent, prop.child) == (root_site.candidate, child_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::SeedOnce(site) if site == prop.site)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        seed_uses.len(),
        1,
        "signal(self.initial) must sample exactly one SeedOnce edge"
    );
    assert_eq!(
        plan.owners.candidate_to_owner[&root_site.candidate],
        plan.owners.candidate_to_owner[&child_site.candidate],
        "a SeedOnce edge still shares activation/lifecycle ownership"
    );
}

fn assert_live_query_key(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("query-key root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == child)
        .expect("query child site");
    assert_eq!(child_site.parent, Some(root_site.candidate));

    let key_dependencies = plan
        .component_inputs
        .live_props
        .iter()
        .filter(|prop| {
            (prop.parent, prop.child) == (root_site.candidate, child_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::QueryKeyDependency)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        key_dependencies.len(),
        1,
        "the inbound topic must remain a normative query-key dependency"
    );
    assert_eq!(
        plan.owners.candidate_to_owner[&root_site.candidate],
        plan.owners.candidate_to_owner[&child_site.candidate],
        "a live query key must stay inside one fused owner"
    );

    let linked_child = plan
        .components
        .iter()
        .find(|component| component.semantic_key == child)
        .expect("canonical query child");
    assert!(
        linked_child.codegen.uses_queries,
        "reachable query work must be reflected in the final codegen bridge"
    );
    let rsjs_ir::abi::DescriptorBody::Component(descriptor) = &linked_child.descriptor.body else {
        panic!("query child descriptor must be a component");
    };
    assert!(
        descriptor
            .origins
            .values()
            .any(|origin| matches!(origin, rsjs_ir::abi::OriginKind::Query)),
        "reachable query work must own a canonical query origin"
    );
    assert!(
        descriptor.effects.values().any(|effect| matches!(
            effect,
            rsjs_ir::abi::EffectKind::QueryObservation
                | rsjs_ir::abi::EffectKind::QueryRefetch
                | rsjs_ir::abi::EffectKind::QueryInvalidation
        )),
        "rendered data and the bound refresh handler must retain canonical query effects"
    );
}

fn assert_linked_mutation_owner(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("mutation root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == child)
        .expect("mutation child site");
    assert_eq!(child_site.parent, Some(root_site.candidate));
    assert!(
        plan.component_inputs.live_props.iter().any(|prop| {
            (prop.parent, prop.child) == (root_site.candidate, child_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::LiveRead)
        }),
        "the imported async mutation child must keep its label live"
    );
    assert_eq!(
        plan.owners.candidate_to_owner[&root_site.candidate],
        plan.owners.candidate_to_owner[&child_site.candidate],
        "the live parent-to-mutation-child edge must stay inside one fused owner"
    );

    let linked_child = plan
        .components
        .iter()
        .find(|component| component.semantic_key == child)
        .expect("canonical mutation child");
    assert!(
        linked_child.codegen.uses_mutations,
        "reachable mutation work must be reflected in the final codegen bridge"
    );
    let rsjs_ir::abi::DescriptorBody::Component(descriptor) = &linked_child.descriptor.body else {
        panic!("mutation child descriptor must be a component");
    };
    assert!(
        descriptor
            .origins
            .values()
            .any(|origin| matches!(origin, rsjs_ir::abi::OriginKind::Mutation)),
        "reachable mutation work must own a canonical mutation origin"
    );
    assert!(
        descriptor
            .effects
            .values()
            .any(|effect| matches!(effect, rsjs_ir::abi::EffectKind::Mutation)),
        "rendered state and the bound run handler must retain canonical mutation effects"
    );
}

fn assert_conditional_component_lifecycle(
    plan: &proa_cli::rsjs::semantic_link::SemanticLinkPlan,
    root: ComponentSemanticKey,
    child: ComponentSemanticKey,
    descendant: ComponentSemanticKey,
) {
    let root_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == root)
        .expect("conditional root site");
    let child_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == child)
        .expect("conditional child site");
    let descendant_site = plan
        .sites
        .iter()
        .find(|site| site.root == root && site.component == descendant)
        .expect("conditional static descendant site");

    let lifecycle = child_site
        .incoming_lifecycle
        .expect("conditional child must retain its exact incoming lifecycle control");
    assert_eq!(
        lifecycle.branch,
        rsjs_ir::abi::ComponentLifecycleBranch::Then
    );
    assert_eq!(child_site.parent, Some(root_site.candidate));
    assert_eq!(
        child_site.reactive_lifecycle_root,
        Some(child_site.candidate),
        "the conditional child must root its own mount/dispose group"
    );
    assert_eq!(descendant_site.parent, Some(child_site.candidate));
    assert_eq!(
        descendant_site.incoming_lifecycle, None,
        "the descendant edge is structurally static inside the conditional child"
    );
    assert_eq!(
        descendant_site.reactive_lifecycle_root,
        Some(child_site.candidate),
        "a static descendant must inherit the conditional child's lifecycle group"
    );

    for site in [child_site, descendant_site] {
        assert_eq!(
            plan.owners.candidate_to_owner[&root_site.candidate],
            plan.owners.candidate_to_owner[&site.candidate],
            "the conditional component graph must remain in one fused owner"
        );
    }

    assert!(
        plan.component_inputs.live_props.iter().any(|prop| {
            (prop.parent, prop.child) == (root_site.candidate, child_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::SeedOnce(site) if site == prop.site)
        }),
        "the conditional child's local signal must sample a fresh SeedOnce value on each mount"
    );
    assert!(
        plan.component_inputs.live_props.iter().any(|prop| {
            (prop.parent, prop.child) == (root_site.candidate, child_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::LiveRead)
        }),
        "the conditional child must retain its parent-owned live value"
    );
    assert!(
        plan.component_inputs.live_callbacks.iter().any(|callback| {
            (callback.parent, callback.child) == (root_site.candidate, child_site.candidate)
        }),
        "the conditional child must resolve the current parent callback on every mount"
    );
    assert!(
        plan.component_inputs.live_props.iter().any(|prop| {
            (prop.parent, prop.child) == (child_site.candidate, descendant_site.candidate)
                && matches!(prop.temporal, TemporalPropMode::LiveRead)
        }),
        "the static descendant must keep the child's live value while the lifecycle group is mounted"
    );
}
