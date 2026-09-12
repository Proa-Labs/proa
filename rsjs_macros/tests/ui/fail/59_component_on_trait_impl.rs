use rsjs::rsjs;

struct Counter;

#[rsjs]
impl<B: proa_core::WriteBuf, L: proa_core::DataLoader> proa_core::WebRenderSync<L, B> for Counter {
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        proa_macros::html_sync! { <span>"counter"</span> }.render(cx)
    }
}

fn main() {}
