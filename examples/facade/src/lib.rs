// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A consumer with a single Proa dependency: no direct implementation crates.

use proa::{DataLoader, MdRenderSync, WebContext, WebRenderSync, WriteError, html_sync, md_sync};

pub struct Greeting<'a> {
    pub name: &'a str,
}

impl<L: DataLoader> WebRenderSync<L> for Greeting<'_> {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <p>{proa::text!("Hello, {}", self.name)}</p>
        }
        .render(cx)
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

// Exercise RSJS expansion with no direct RSJS or implementation-crate dependency.
#[derive(proa::rsjs::ClientProps)]
pub struct ClientGreeting {
    pub name: String,
}

#[derive(proa::rsjs::SharedState)]
#[rsjs(type_key = "facade.counter")]
pub struct CounterState {
    pub count: i64,
}

#[proa::rsjs::client]
pub fn doubled(value: i64) -> i64 {
    value * 2
}

#[proa::rsjs::client_module]
pub mod helpers {
    #[proa::rsjs::client]
    pub fn increment(value: i64) -> i64 {
        value + 1
    }
}

pub struct StaticIsland;

#[proa::rsjs::rsjs(component)]
impl<L: DataLoader> WebRenderSync<L> for StaticIsland {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <span data-facade-island>"Ready"</span>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa::{actions::ActionHandler, axum::RouteResponse};

    use proa::rsjs;

    struct Counter {
        initial: i64,
    }

    #[rsjs::rsjs(component, client)]
    impl<L: DataLoader> WebRenderSync<L> for Counter {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            let count = rsjs::signal::<i64>(self.initial);
            let increment = rsjs::event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| {
                count.update(|value| value + 1)
            });
            html_sync! {
                <button type="button" on_click={increment()}>{count.get()}</button>
            }
            .render(cx)
        }
    }

    #[test]
    fn facade_client_component_renders_initial_state() {
        let output = proa::render_to_string(Counter { initial: 37 });
        assert!(output.contains(">37</button>"));
        assert!(output.contains("data-rsjs"));
    }

    #[test]
    fn facade_rsjs_codecs_and_helpers_work() {
        use proa::rsjs::{ClientDecode, ClientEncode};
        let input = ClientGreeting {
            name: "<Ada>".into(),
        };
        let mut encoded = String::new();
        input.encode_into(&mut encoded);
        let decoded = ClientGreeting::decode(&encoded).unwrap();
        assert_eq!(decoded.name, input.name);
        assert_eq!(doubled(21), 42);
        assert_eq!(helpers::increment(41), 42);
        assert_eq!(
            proa::render_to_string(StaticIsland),
            "<span data-facade-island>Ready</span>"
        );
    }

    #[test]
    fn facade_templates_escape_dynamic_values() {
        let html = proa::render_to_string(Greeting { name: "<Ada>" });
        assert_eq!(html, "<p>Hello, &lt;Ada&gt;</p>");
        let mut cx = WebContext::new();
        Greeting { name: "*Ada*" }.render_md(&mut cx).unwrap();
        let markdown = cx.into_output().into_string();
        assert!(markdown.contains("\\*Ada\\*"));
        let name = "<&>";
        let xml = proa::render_to_string(proa::xml_sync! {
            <name>{name}</name>
        });
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
        let html = RouteResponse::ssr_async(proa::html! {
            <p>{name}</p>
        })
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

#[cfg(test)]
mod divergent_loops {
    use proa::{MdRenderSync, NoLoader, WebContext, WriteError};

    fn next_row(count: &mut usize) -> Result<(), WriteError> {
        *count += 1;
        if *count > 1 {
            Err(WriteError::OutOfCapacity)
        } else {
            Ok(())
        }
    }

    #[test]
    #[allow(unreachable_code)]
    fn nested_facade_templates_preserve_partial_output_on_loop_error() {
        let mut cx = WebContext::<NoLoader>::new();
        let result = MdRenderSync::render_md(
            proa::md_sync! { r#"{ {
            let mut count = 0;
            loop { next_row(&mut count)?; ::proa::md_sync! { "row" } }
        } }"# },
            &mut cx,
        );
        assert_eq!(result, Err(WriteError::OutOfCapacity));
        assert_eq!(cx.into_output().into_string(), "row");

        let mut cx = WebContext::<NoLoader>::new();
        let result = MdRenderSync::render_md(
            proa::md_sync! { r#"{ {
            let mut count = 0;
            loop {
                if count == 1 { return Err(WriteError::OutOfCapacity); }
                count += 1;
                ::proa::md_sync! { "row" }
            }
        } }"# },
            &mut cx,
        );
        assert_eq!(result, Err(WriteError::OutOfCapacity));
        assert_eq!(cx.into_output().into_string(), "row");
    }

    #[test]
    #[allow(unreachable_code)]
    fn nested_async_facade_templates_preserve_partial_output_on_loop_error() {
        let mut cx = WebContext::<NoLoader>::new();
        let result = futures::executor::block_on(
            proa::MdRender::render_md(
                proa::md! { r#"{ {
            let mut count = 0;
            loop { next_row(&mut count)?; ::proa::md! { "row" } }
        } }"# },
                &mut cx,
            )
            .resolve(),
        );
        assert_eq!(result, Err(WriteError::OutOfCapacity));
        assert_eq!(cx.into_output().into_string(), "row");
    }
}
