#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ValueIfMissingElse;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ValueIfMissingElse {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let active = signal(true);

        proa_macros::html_sync! {
            <div><span>{if active.get() { "yes" }}</span></div>
        }
        .render(cx)
    }
}

fn main() {}
