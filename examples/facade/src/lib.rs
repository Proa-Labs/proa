// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A consumer with a single Proa dependency: no direct implementation crates.

use proa::{html_sync, md_sync, DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteError};

pub struct Greeting<'a> {
    pub name: &'a str,
}

impl<L: DataLoader> WebRenderSync<L> for Greeting<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! { <p>{proa::text!("Hello, {}", self.name)}</p> }.render(cx)
    }
}

impl<L: DataLoader> MdRenderSync<L> for Greeting<'_> {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        md_sync! { "Hello, {self.name}" }.render_md(cx)
    }
}

#[derive(proa::ProaComponent)]
#[component(tag = "section", children_builder)]
pub struct Panel<C = ()> {
    #[component(children)]
    pub children: Option<C>,
}

impl<C> Panel<C> {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}

#[derive(proa::Action)]
#[action(scope = "example")]
pub enum MessageAction {
    Send { recipient: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa::{actions::ActionHandler, axum::RouteResponse};

    #[test]
    fn facade_templates_escape_dynamic_values() {
        let html = proa::render_to_string(Greeting { name: "<Ada>" });
        assert_eq!(html, "<p>Hello, &lt;Ada&gt;</p>");
        let mut cx = WebContext::new();
        Greeting { name: "*Ada*" }.render_md(&mut cx).unwrap();
        let markdown = cx.into_output().into_string();
        assert!(markdown.contains("\\*Ada\\*"));
        let name = "<&>";
        let xml = proa::render_to_string(proa::xml_sync! { <name>{name}</name> });
        assert_eq!(xml, "<name>&lt;&amp;&gt;</name>");
    }

    #[test]
    fn facade_tree_macro_uses_reexported_ir() {
        let tree = proa::html_tree! { <main><Widget /></main> };
        assert!(tree.uses_component("Widget"));
    }

    #[test]
    fn facade_async_templates_finalize_without_http() {
        let name = "<Ada>";
        let html = RouteResponse::ssr_async(proa::html! { <p>{name}</p> })
            .render_to_string()
            .unwrap();
        assert_eq!(html, "<p>&lt;Ada&gt;</p>");
    }

    #[test]
    fn facade_derived_component_composes_generic_children() {
        let panel = Panel {
            children: Some(Greeting { name: "<Ada>" }),
        };
        assert_eq!(
            proa::render_to_string(panel),
            "<section><p>Hello, &lt;Ada&gt;</p></section>"
        );
    }

    #[test]
    fn facade_async_markdown_escapes_dynamic_values() {
        let name = "*Ada*";
        let markdown = futures::executor::block_on(proa::render_md_to_string_async(
            proa::md! { "Hello, {name}" },
        ));
        assert!(markdown.contains("\\*Ada\\*"));
    }

    #[test]
    fn facade_action_derive_round_trips_payloads() {
        let action = MessageAction::Send {
            recipient: "Ada".into(),
        };
        let data = action.to_action_data();
        let MessageAction::Send { recipient } = MessageAction::from_action_data(data).unwrap();
        assert_eq!(recipient, "Ada");
    }
}
