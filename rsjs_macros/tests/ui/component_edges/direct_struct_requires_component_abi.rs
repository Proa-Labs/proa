use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

struct UndecoratedDirectEdge;

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for UndecoratedDirectEdge {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <span data-undecorated-direct-edge></span>
        }
        .render(cx)
    }
}

struct Parent;

#[rsjs::rsjs(component)]
impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for Parent {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section>{UndecoratedDirectEdge {}}</section>
        }
        .render(cx)
    }
}

fn main() {}
