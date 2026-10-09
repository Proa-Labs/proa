#![allow(unused_imports)]
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};

pub struct SmallSigned8 { pub delta: i8 }
pub struct SmallSigned16 { pub delta: i16 }

#[rsjs::rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for SmallSigned8 {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let delta: rsjs::Signal<i8> = rsjs::signal(self.delta);
        proa_macros::html_sync! { <p>{proa_macros::text!("{:x}", delta.get())}</p> }.render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for SmallSigned16 {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let delta: rsjs::Signal<i16> = rsjs::signal(self.delta);
        proa_macros::html_sync! { <p>{proa_macros::text!("{:o}", delta.get())}</p> }.render(cx)
    }
}

fn main() {}
