#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct DebouncedHandler;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for DebouncedHandler {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input on_input={rsjs::debounce(300, |event| query.set(event.value()))} />
                <span>{query.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
