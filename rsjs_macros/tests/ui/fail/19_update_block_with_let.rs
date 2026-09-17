// `signal.update(|c| { let x = 5; c + x })` used to silently drop
// the `let x = 5;` binding because `lower_simple_closure` kept only
// `block.stmts.last()`. The analyser then either rejected the body
// with the confusing "unknown signal `x`" message or silently
// misanalysed the binding. Round-4 review #5: reject explicitly with
// a span-pointed error at the offending `let`.

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

impl<T: Clone + std::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }
    pub fn set(&self, _v: T) {}
    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}

pub struct Counter;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        proa_macros::html_sync! {
            <div>
                <button on_click={|_| count.update(|c| { let x = 5_i64; c + x })}>"+5"</button>
                <span>{count.get()}</span>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
