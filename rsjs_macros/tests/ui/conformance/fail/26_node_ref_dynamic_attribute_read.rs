#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NodeRefDynamicAttributeRead;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NodeRefDynamicAttributeRead {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
        let attr = signal("data-kind".to_string());
        let value = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input ref={input}/>
                <button on_click={|_| value.set(input.get_attribute(attr.get()))}> "Read" </button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
