#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::WebRender;
use rsjs::rsjs;
use support::*;

pub struct AsyncRenderMethodGenerics;

#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for AsyncRenderMethodGenerics {
    fn render<'x>(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let status = signal("");

            proa_macros::html! {
                <div>{status.get()}</div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn main() {}
