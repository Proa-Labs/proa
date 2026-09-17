#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NodeRefDynamicAttributeName;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NodeRefDynamicAttributeName {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
        let attr = signal("aria-expanded".to_string());

        proa_macros::html_sync! {
            <div>
                <input ref={input}/>
                <button on_click={|_| input.set_attribute(attr.get(), "true")}> "Expand" </button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
