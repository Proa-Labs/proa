#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerRefArity;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for HandlerRefArity {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let save = rsjs::handler(|| count.set(1_i64));

        proa_macros::html_sync! {
            <div><button on_click={save(())}>"Save"</button></div>
        }
        .render(cx)
    }
}

fn main() {}
