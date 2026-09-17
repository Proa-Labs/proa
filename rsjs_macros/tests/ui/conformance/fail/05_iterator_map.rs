#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct IteratorMap;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for IteratorMap {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let values = [1_i64, 2_i64];

        proa_macros::html_sync! {
            <button on_click={|_| {
                count.set(values.map(|value| value + 1_i64));
            }}>
                "Apply"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
