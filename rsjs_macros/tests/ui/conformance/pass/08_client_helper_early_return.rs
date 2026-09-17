#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

#[rsjs::client_module]
pub mod pricing_labels {
    #[rsjs::client]
    pub fn label(price: i64) -> String {
        if price > 100 {
            return "premium".to_string();
        }
        "standard".to_string()
    }
}

pub struct HelperEarlyReturn;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HelperEarlyReturn {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let price = signal(100_i64);

        proa_macros::html_sync! {
            <button on_click={|_| price.set(price.get() + 1)}>
                {pricing_labels::label(price.get())}
            </button>
        }
        .render(cx)
    }
}

fn main() {}
