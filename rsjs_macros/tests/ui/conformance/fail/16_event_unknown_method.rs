#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct EventUnknownMethod;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for EventUnknownMethod {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input on_input={|event| value.set(event.native_event())}/>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
