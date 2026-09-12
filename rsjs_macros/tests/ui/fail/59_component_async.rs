use rsjs::rsjs;

struct Counter;

#[rsjs(component, client)]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRender<L, B> for Counter {
    async fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> Result<(), proa_core::WriteError> {
        proa_macros::html! { <span>"counter"</span> }
            .render(cx)
            .resolve()
            .await
    }
}

fn main() {}
