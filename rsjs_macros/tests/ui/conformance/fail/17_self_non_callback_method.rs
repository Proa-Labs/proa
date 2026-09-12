#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct SelfNonCallbackMethod;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for SelfNonCallbackMethod {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| self.save_now(())}>"Save"</button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
