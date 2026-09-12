#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerDynamicRangeForLoop;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for HandlerDynamicRangeForLoop {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| {
                for value in count.get()..3_i64 {
                    count.set(value);
                }
            }}>
                "Apply"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
