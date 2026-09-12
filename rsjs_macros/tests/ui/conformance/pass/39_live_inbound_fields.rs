#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct LiveInboundFields {
    pub label: String,
    pub class: String,
    pub enabled: bool,
    pub on_activate: rsjs::Handler<String>,
}

#[rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for LiveInboundFields {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let activate = rsjs::event_handler(|_: rsjs::Event| {
            self.on_activate.call(self.label);
        });

        proa_macros::html_sync! {
            <section class={self.class}>
                <p>{self.label}</p>
                {if self.enabled {
                    proa_macros::html_sync! {
                        <button type="button" on_click={activate()}>
                            "Activate"
                        </button>
                    }
                }}
            </section>
        }
        .render(cx)
    }
}

mod inbound_query_key {
    use super::*;
    use proa_core::WebRender;

    pub struct InboundQueryKey {
        pub resource: String,
    }

    #[rsjs(component, client)]
    impl<B: WriteBuf, L: proa_core::DataLoader> WebRender<L, B> for InboundQueryKey {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> proa_core::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>>
        {
            proa_core::RenderOutcome::pending(async move {
                let resource: rsjs::Query<String> = rsjs::query_with_initial(
                    rsjs::QueryKey::new(("resource", self.resource)),
                    String::new(),
                    "/api/resource",
                    rsjs::QueryOptions::default(),
                );

                proa_macros::html! {
                    <section>
                        <span>{self.resource}</span>
                        <output>{resource.data()}</output>
                    </section>
                }
                .render(cx)
                .resolve()
                .await
            })
        }
    }
}

fn main() {}
