#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use proa_core::WebRender;
use rsjs::{rsjs, QueryKey};
use support::*;

pub struct QueryStreamTextRequiresTextResponse;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRender<L, B>
    for QueryStreamTextRequiresTextResponse
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let feed: rsjs::Query<&'static str> = rsjs::query_with_initial(
                QueryKey::new(("feed",)),
                "",
                "/api/feed",
                rsjs::QueryOptions {
                    stream_text: true,
                    ..Default::default()
                },
            );

            proa_macros::html! {
                <div>{feed.data().as_str()}</div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn main() {}
