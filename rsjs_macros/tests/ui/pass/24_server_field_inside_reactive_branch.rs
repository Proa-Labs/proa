// Server-owned expressions remain opaque SSR even when they appear inside a
// client-controlled branch. Only the branch predicate participates in Client
// IR; the server field does not need a server_only(...) wrapper.

#[allow(unused_imports)]
use proa_core::{WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::rsjs;

#[allow(dead_code)]
fn signal<T>(initial: T) -> Signal<T> {
    Signal { initial }
}

#[allow(dead_code)]
struct Signal<T> {
    initial: T,
}

impl<T: Clone + core::fmt::Display> Signal<T> {
    pub fn get(&self) -> T {
        self.initial.clone()
    }
}

pub struct RowMenu<'a> {
    pub ticket_id: &'a str,
}

fn format_ticket(ticket_id: &str) -> String {
    ticket_id.to_owned()
}

#[rsjs(component, client)]
impl<'a, B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for RowMenu<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let open = signal(true);
        proa_macros::html_sync! {
            <div>
                {if open.get() {
                    proa_macros::html_sync! {
                        <p>{format_ticket(self.ticket_id)}</p>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}

fn main() {}
