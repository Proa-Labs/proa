#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct DebouncedHandler;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DebouncedHandler {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input on_input={rsjs::debounce(|event| query.set(event.value()), 300)} />
                <span>{query.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
