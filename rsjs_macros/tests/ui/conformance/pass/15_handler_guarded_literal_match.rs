#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerGuardedLiteralMatch;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerGuardedLiteralMatch {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| match count.get() {
                0_i64 if count.get() == 0_i64 => count.set(1_i64),
                _ => count.set(2_i64),
            }}>
                "Toggle"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
