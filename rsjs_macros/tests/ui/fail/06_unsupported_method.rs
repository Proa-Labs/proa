// Category 6: `#[rsjs(component, client)]` rejects a reactive expression that calls a
// method not in the v0.1 allowlist.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> { Signal { initial } }
#[allow(dead_code)]
struct Signal<T> { initial: T }
impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T { self.initial.clone() }
    pub fn frobnicate(&self) -> T { self.initial.clone() }
}

pub struct Counter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div><span>{count.frobnicate()}</span></div>
        }
        .render(cx)
    }
}

fn main() {}
