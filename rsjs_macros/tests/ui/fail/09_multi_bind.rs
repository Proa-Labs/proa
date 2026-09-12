// Category 9: `#[rsjs(component, client)]` rejects more than one `bind_*` attribute on
// the same element.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
}

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let a = signal(String::new());
        let b = signal(false);
        proa_macros::html_sync! {
            <div><input bind_value={a} bind_checked={b}/></div>
        }
        .render(cx)
    }
}

fn main() {}
