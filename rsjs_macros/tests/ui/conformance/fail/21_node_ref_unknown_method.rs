#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NodeRefUnknownMethod;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for NodeRefUnknownMethod {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();

        proa_macros::html_sync! {
            <div>
                <input ref={input}/>
                <button on_click={|_| input.scroll_to_top()}> "Scroll" </button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
