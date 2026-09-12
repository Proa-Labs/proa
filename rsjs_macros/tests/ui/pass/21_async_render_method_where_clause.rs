// The raw `WebRender::render` signature, including an authored method where
// clause, is preserved by `#[rsjs(component, client)]`.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRender, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

#[allow(dead_code)]
impl<T: Clone + std::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }
}

pub struct AsyncWhere;

#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for AsyncWhere {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    where
        Self: Send,
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
