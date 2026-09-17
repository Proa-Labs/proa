#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerStaticForLoop;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerStaticForLoop {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| {
                for value in [1_i64, 2_i64] {
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
