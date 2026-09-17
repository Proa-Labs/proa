#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerPropChild {
    pub on_change: rsjs::Handler<()>,
}

impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerPropChild {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _ = self.on_change;

        proa_macros::html_sync! {
            <button type="button">"Change"</button>
        }
        .render(cx)
    }
}

pub struct HandlerPropWithoutInlining;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerPropWithoutInlining {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let saved = signal(false);
        let on_change = rsjs::handler(|_| saved.set(true));

        proa_macros::html_sync! {
            <div>
                {HandlerPropChild { on_change }}
            </div>
        }
        .render(cx)
    }
}

fn main() {}
