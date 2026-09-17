#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct NodeRefs;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for NodeRefs {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let input = rsjs::node_ref::<rsjs::HtmlInputElement>();
        let form = rsjs::node_ref::<rsjs::HtmlFormElement>();
        let dialog = rsjs::node_ref::<rsjs::HtmlDialogElement>();
        let popover = rsjs::node_ref::<rsjs::HtmlElement>();
        let current = signal(String::new());

        proa_macros::html_sync! {
            <div>
                <input ref={input} />
                <form ref={form}></form>
                <dialog ref={dialog}>"Dialog"</dialog>
                <div popover="" ref={popover}>"Popover"</div>
                <button on_click={|_| {
                    input.focus();
                    input.click();
                    form.request_submit();
                    form.reset();
                    dialog.show_modal();
                    dialog.close();
                    popover.show_popover();
                    popover.hide_popover();
                    popover.toggle_popover();
                    input.scroll_to(0_i64, 32_i64);
                    input.scroll_by(0_i64, 8_i64);
                    input.select_all();
                    input.set_value("");
                    input.set_selection_range(0_i64, 0_i64);
                    input.set_attribute("aria-expanded", "false");
                    input.remove_attribute("hidden");
                    input.set_style_property("outline-color", "transparent");
                    console::log(input.get_attribute("data-kind"), input.has_attribute("hidden"));
                    current.set(input.value());
                }}>
                    "Clear"
                </button>
                <span>{current.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
