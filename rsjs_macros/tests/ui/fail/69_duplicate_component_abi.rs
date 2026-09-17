// A semantic component type may carry only one #[rsjs(component...)] render
// impl, even when it implements both Proa render traits.

use proa_core::{DataLoader, RenderOutcome, WebContext, WriteBuf, WriteError};
use rsjs::rsjs;

struct Duplicate;

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> proa_core::WebRenderSync<L, B> for Duplicate {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        #[allow(unused_imports)]
        use proa_core::WebRenderSync as _;

        proa_macros::html_sync! {
            <span>"sync"</span>
        }
        .render(cx)
    }
}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> proa_core::WebRender<L, B> for Duplicate {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::pending(async move {
            #[allow(unused_imports)]
            use proa_core::WebRender as _;

            proa_macros::html! {
                <span>"async"</span>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn main() {}
