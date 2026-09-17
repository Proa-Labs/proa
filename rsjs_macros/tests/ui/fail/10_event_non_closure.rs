// Category 10 (event handler shape): `on_*` attributes require a
// closure value. Passing a non-closure expression (here a signal name)
// is rejected because the v0.1 lowering needs a closure body to
// translate to a Client IR v3 handler block.

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
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div><button on_click={count}>"x"</button></div>
        }
        .render(cx)
    }
}

fn main() {}
