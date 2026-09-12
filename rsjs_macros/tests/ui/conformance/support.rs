#![allow(dead_code)]
#![allow(unused_imports)]

pub use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};

pub fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

pub struct Signal<T> {
    initial: T,
}

// `Clone` is the only bound the stub methods need; keyed-list fixtures
// hold `Vec<String>` signals, which have no `Display`.
impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }

    pub fn set(&self, _value: T) {}

    pub fn update<F: Fn(T) -> T>(&self, _f: F) {}
}
