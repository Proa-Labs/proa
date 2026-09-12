#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct RefInTemplateLoop;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for RefInTemplateLoop {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let item = rsjs::node_ref::<rsjs::HtmlElement>();

        proa_macros::html_sync! {
            <ul>
                {for _idx in 0..2 {
                    proa_macros::html_sync! {
                        <li ref={item}>"Row"</li>
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}

fn main() {}
