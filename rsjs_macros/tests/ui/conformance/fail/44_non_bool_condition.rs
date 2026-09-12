#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NonBoolCondition;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for NonBoolCondition {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| {
                if count.get() {
                    count.set(1_i64);
                }
            }}>
                "Invalid truthiness"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
