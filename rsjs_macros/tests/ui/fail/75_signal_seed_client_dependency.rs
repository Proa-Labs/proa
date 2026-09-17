#![allow(unused_imports)]

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct DependentSignalSeed;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DependentSignalSeed {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let first = rsjs::signal(1_i64);
        let second = rsjs::signal(first.get() + 1_i64);

        proa_macros::html_sync! {
            <output>{second.get()}</output>
        }
        .render(cx)
    }
}

fn main() {}
