// Dynamic second-order HTML / multi-URL attributes stay rejected. Ordinary
// reactive attrs such as `class`, `aria-*`, `data-*`, sanitized single URL
// attrs, and boolean attrs are covered by parser_attr_bindings.rs.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }
}

pub struct DynamicHref;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for DynamicHref {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let html = signal("<script>alert(1)</script>");
        proa_macros::html_sync! {
            <iframe srcdoc={html.get()}></iframe>
        }
        .render(cx)
    }
}

fn main() {}
