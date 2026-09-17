#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use rsjs::web::dom;
use support::*;

pub struct RepeatedGlobalEvents;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for RepeatedGlobalEvents {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let labels = ["A", "B"];

        proa_macros::html_sync! {
            <ul>
                {for label in labels {
                    proa_macros::html_sync! {
                        <li
                            on_document_keydown={|event: rsjs::KeyboardEvent<dom::Document>| {
                                console::log(label.to_string(), event.key());
                            }}
                            on_window_resize={|_: rsjs::Event<dom::Window>| {
                                console::log(label.to_string());
                            }}
                        >
                            {rsjs::server_only(label)}
                        </li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

fn main() {}
