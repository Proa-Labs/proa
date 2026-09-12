#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct BoundaryHandlerChild<F> {
    pub on_press: F,
}

pub struct IslandBoundaryHandlerProp;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for IslandBoundaryHandlerProp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let pressed = signal(0_i64);
        let on_press = rsjs::handler(|_| pressed.set(1_i64));

        proa_macros::html_sync! {
            <section>
                {rsjs::island(BoundaryHandlerChild { on_press })}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
