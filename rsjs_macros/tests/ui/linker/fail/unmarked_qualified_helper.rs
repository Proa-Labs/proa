// Link-stage fixture: Rust resolves this ordinary function, but descriptor
// linking must reject it because it has no `#[rsjs::client]` definition.

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

mod pricing {
    pub fn subtotal(value: i64, quantity: i64) -> i64 {
        value * quantity
    }
}

pub struct Counter;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div><span>{pricing::subtotal(count.get(), 2)}</span></div>
        }
        .render(cx)
    }
}

fn main() {}
