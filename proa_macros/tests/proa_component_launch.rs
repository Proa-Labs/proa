// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Launch-facing authoring fixture for the common single-root component path.
//!
//! Keep this example free of render buffers, data loaders, render lifetimes,
//! and sync-to-async adapters. Those details belong in generated code.
//! The literal interactive companion lives in
//! `rsjs_macros/tests/explicit_trait_component.rs`.

use proa_actions::ActionDataStatic;
use proa_macros::{html, ProaComponent};

const INCREMENT: ActionDataStatic =
    ActionDataStatic::with_params("counter.increment", r#"{"step":1}"#);

#[derive(ProaComponent)]
#[component(tag = "main", async_render, children_builder)]
struct LaunchPage<C = ()> {
    #[component(attr, attr_name = "data-page")]
    page: Option<&'static str>,
    #[component(children)]
    children: Option<C>,
}

impl<C> LaunchPage<C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["launch-page"], 1)
    }
}

#[derive(ProaComponent)]
#[component(tag = "section", async_render, children_builder)]
struct FeaturePanel<C = ()> {
    #[component(attr, attr_name = "aria-label")]
    label: Option<&'static str>,
    #[component(children)]
    children: Option<C>,
}

impl<C> FeaturePanel<C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["feature-panel"], 1)
    }
}

#[derive(ProaComponent)]
#[component(tag = "button", children_builder)]
struct ActionButton<C = ()> {
    #[component(attr, attr_name = "type")]
    button_type: Option<&'static str>,
    #[component(action, attr_name = "data-action")]
    action: Option<ActionDataStatic>,
    #[component(children)]
    children: Option<C>,
}

impl<C> ActionButton<C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["action-button"], 1)
    }
}

fn launch_demo() -> LaunchPage<FeaturePanel<ActionButton<&'static str>>> {
    LaunchPage::<()> {
        page: Some("counter"),
        children: None,
    }
    .with_children(
        FeaturePanel::<()> {
            label: Some("Counter controls"),
            children: None,
        }
        .with_children(
            ActionButton::<()> {
                button_type: Some("button"),
                action: Some(INCREMENT),
                children: None,
            }
            .with_children("Increment"),
        ),
    )
}

#[test]
fn nested_components_render_optional_children_and_typed_action_descriptor() {
    let rendered = proa_core::render_to_string(launch_demo());

    assert_eq!(
        rendered,
        concat!(
            r#"<main class="launch-page" data-page="counter">"#,
            r#"<section class="feature-panel" aria-label="Counter controls">"#,
            r#"<button class="action-button" type="button" "#,
            r#"data-action="counter.increment" data-param-step="1">"#,
            "Increment</button></section></main>",
        )
    );
}

#[tokio::test]
async fn nested_sync_components_compose_directly_in_an_async_template() {
    let rendered = proa_core::render_to_string_async(html! {
        <div data-demo="async">{launch_demo()}</div>
    })
    .await;

    assert!(rendered.starts_with(r#"<div data-demo="async"><main class="launch-page""#));
    assert!(rendered.ends_with("</main></div>"));
}
