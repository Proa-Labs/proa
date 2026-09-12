#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ChildWithServerValue<V> {
    pub value: V,
}

impl<B: WriteBuf, L: proa_core::DataLoader, V> WebRenderSync<L, B> for ChildWithServerValue<V> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <span>"Child"</span>
        }
        .render(cx)
    }
}

pub struct IslandStateDeclaration;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for IslandStateDeclaration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>
                {rsjs::island(ChildWithServerValue { value: signal(0_i64).get() })}
            </div>
        }
        .render(cx)
    }
}

fn main() {}
