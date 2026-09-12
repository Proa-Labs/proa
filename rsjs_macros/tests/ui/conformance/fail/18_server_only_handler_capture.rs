#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct ServerOnlyChild<F> {
    pub on_change: F,
}

impl<B: WriteBuf, L: proa_core::DataLoader, F> WebRenderSync<L, B> for ServerOnlyChild<F> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <button type="button">"Child"</button>
        }
        .render(cx)
    }
}

pub struct ServerOnlyHandlerCapture;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ServerOnlyHandlerCapture {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let saved = signal(false);
        let on_change = rsjs::handler(|_| saved.set(true));

        proa_macros::html_sync! {
            <div>
                {rsjs::server_only(ServerOnlyChild { on_change })}
            </div>
        }
        .render(cx)
    }
}

fn main() {}
