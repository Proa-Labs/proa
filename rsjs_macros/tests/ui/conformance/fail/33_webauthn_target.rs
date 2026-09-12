#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct WebAuthnTarget;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for WebAuthnTarget {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let options = signal(r#"{"challenge":"auth"}"#.to_string());
        let credential = signal(String::new());

        proa_macros::html_sync! {
            <button on_click={|_| {
                rsjs::webauthn::get_json_into(options.get(), credential.get());
            }}>
                "Login"
            </button>
        }
        .render(cx)
    }
}

fn main() {}
