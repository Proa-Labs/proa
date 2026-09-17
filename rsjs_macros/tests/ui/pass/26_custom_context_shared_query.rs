use proa_core::{DataLoader, WebContext, WebRender, WriteBuf, WriteError};
use rsjs::rsjs;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "proa.tests.custom-context")]
pub struct PageState {
    pub count: i64,
}

pub struct CustomContext;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for CustomContext {
    fn render(
        self,
        context: &mut WebContext<L, B>,
    ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
    {
        proa_core::RenderOutcome::pending(async move {
            let page = rsjs::shared::<PageState>();
            let count = page.count();
            let query: rsjs::Query<String> = rsjs::query_with_initial(
                rsjs::QueryKey::new(("custom-context",)),
                String::from("ready"),
                "/api/status",
                rsjs::QueryOptions::default(),
            );

            proa_macros::html! {
                <button
                    data_count={count.get()}
                    on_click={|_| {
                    count.set(1_i64);
                    query.refetch();
                }}
                >
                    "Refresh"
                </button>
            }
            .render(context)
            .resolve()
            .await
        })
    }
}

fn main() {}
