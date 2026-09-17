#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ChildWithServerValue<V> {
    pub value: V,
}

impl<L: proa_core::DataLoader, B: WriteBuf, V> WebRenderSync<L, B> for ChildWithServerValue<V> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <span>"Child"</span>
        }
        .render(cx)
    }
}

pub struct IslandSignalCapture;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for IslandSignalCapture {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let selected = signal(0_i64);

        proa_macros::html_sync! {
            <div>
                {rsjs::island(ChildWithServerValue { value: selected.get() })}
            </div>
        }
        .render(cx)
    }
}

fn main() {}
