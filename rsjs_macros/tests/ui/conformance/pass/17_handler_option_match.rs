#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerOptionMatch;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for HandlerOptionMatch {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let maybe = signal(Option::<i64>::None);
        let count = signal(0_i64);

        proa_macros::html_sync! {
            <button on_click={|_| match maybe.get() {
                Some(_) => count.set(1_i64),
                None => count.set(2_i64),
                _ => count.set(3_i64),
            }}>
                "Toggle"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
