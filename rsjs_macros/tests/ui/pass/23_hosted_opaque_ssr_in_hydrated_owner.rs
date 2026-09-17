use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

pub struct SearchPanel;

fn server_label_attr() -> &'static str {
    "server"
}

fn render_markdown() -> &'static str {
    "server document"
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SearchPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = rsjs::signal(0_i64);
        proa_macros::html_sync! {
            <main data-source={server_label_attr()}>
                <article>{render_markdown()}</article>
                <button on_click={|_| count.update(|value| value + 1)}>{count.get()}</button>
            </main>
        }
        .render(cx)
    }
}

fn main() {}
