use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};

pub struct UnsupportedTextFormat;

#[rsjs::rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for UnsupportedTextFormat {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let amount = rsjs::signal(1.25_f64);
        proa_macros::html_sync! { <p>{proa_macros::text!("{:.2}", amount.get())}</p> }.render(cx)
    }
}

fn main() {}
