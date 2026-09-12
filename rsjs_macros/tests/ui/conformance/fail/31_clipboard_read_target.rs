#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ClipboardReadTarget;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ClipboardReadTarget {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let value = signal(String::new());

        proa_macros::html_sync! {
            <button on_click={|_| {
                rsjs::clipboard::read_text_into(value.get());
            }}>
                "Read"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
