#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::{WebContext, WriteBuf, WriteError};

pub struct AsyncChild;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRender<L, B> for AsyncChild {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            use proa_core::WebRender as _;

            proa_macros::html! {
                <span>"Async child"</span>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

pub struct Parent;

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        use proa_core::WebRenderSync as _;

        proa_macros::html_sync! {
            <section>
                {AsyncChild {}}
            </section>
        }
        .render(cx)
    }
}

fn main() {}
