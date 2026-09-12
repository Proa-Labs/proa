#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ServerDecoratedButton {
    pub label: &'static str,
    pub class: &'static str,
    pub on_press: rsjs::Handler<()>,
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ServerDecoratedButton {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button
                type="button"
                class={rsjs::server_attr(self.class)}
                on_click={|_| self.on_press.call(())}
            >
                {rsjs::server_only(self.label)}
            </button>
        }
        .render(cx)
    }
}

pub struct Parent;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let pressed = signal(0_i64);
        let on_press = rsjs::handler(|_| pressed.set(1_i64));

        proa_macros::html_sync! {
            <section>
                {ServerDecoratedButton {
                    label: "Press",
                    class: "primary",
                    on_press,
                }}
                {rsjs::island(ServerDecoratedButton {
                    label: "Nested island",
                    class: "secondary",
                    on_press: rsjs::noop_handler(),
                })}
                <p>{pressed.get()}</p>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
