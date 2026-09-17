#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct TextInput {
    pub value: String,
    pub on_change: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for TextInput {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let current = signal(self.value);

        proa_macros::html_sync! {
            <label>
                <span>{current.get()}</span>
                <input
                    value={current.get()}
                    on_input={|event| {
                    let value = event.value();
                    console::log(value);
                    self.on_change.call(value);
                }}
                />
            </label>
        }
        .render(cx)
    }
}

pub struct OptionalTextInput {
    pub on_change: Option<rsjs::Handler<String>>,
}

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for OptionalTextInput {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <input on_input={|event| {
                self.on_change.call_if_some(event.value());
            }} />
        }
        .render(cx)
    }
}

pub struct Parent;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());
        let on_change = rsjs::handler(|next: String| value.set(next));

        proa_macros::html_sync! {
            <section>
                {TextInput {
                    value: value.get(),
                    on_change,
                }}
                {OptionalTextInput {
                    on_change: Some(on_change),
                }}
                <p>{value.get()}</p>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
