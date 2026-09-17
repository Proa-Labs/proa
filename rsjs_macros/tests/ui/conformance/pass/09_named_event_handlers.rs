#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NamedEventHandlerRefs;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NamedEventHandlerRefs {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let picked = signal(String::new());
        let logged = signal(String::new());
        let on_pick = rsjs::event_handler(|event: rsjs::Event, label: &'static str| {
            console::log(label);
            logged.set(label);
            picked.set(event.closest_attr("data-date"));
        });

        proa_macros::html_sync! {
            <section>
                <button data_date="sample-day" on_click={on_pick("today")}>"Pick"</button>
                <span>{picked.get()}</span>
                <span>{logged.get()}</span>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
