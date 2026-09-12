// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! SSR contract for the values a shared-state island renders before hydration.
//!
//! Everything a shared field feeds through the analyzed template must appear
//! in the server HTML: the page is meaningful without JavaScript, crawlers see
//! the seeded content, and a form submitted before hydration carries the
//! values the server itself seeded. Ordinary server statements deliberately
//! cannot read compiler-owned shared handles; that would create a hidden
//! client-reactive execution surface.

use proa_core::{Ctx, NoLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(Clone, rsjs::ClientProps)]
pub struct EmitRow {
    pub total: i64,
    pub label: String,
}

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.emit.projection")]
pub struct ProjectionEmitState {
    pub count: i64,
    pub row: EmitRow,
    pub totals: Vec<i64>,
}

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.emit.bind")]
pub struct BindEmitState {
    pub narrow: u32,
    pub name: String,
    pub flag: bool,
}

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "test.emit.prelude-order")]
pub struct PreludeOrderEmitState {
    pub button_bg: String,
}

struct ProjectionPanel;

struct BindPanel;

struct PreludeOrderPanel;

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for ProjectionPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<ProjectionEmitState>();
        let count = state.count();
        let row = state.row();
        let totals = state.totals();

        proa_macros::html_sync! {
            <div on_click={|_| count.update(|value| value + 1_i64)}>
                <span>{count.get()}</span>
                <span>{row.get().label}</span>
                <span>{totals.get()[0]}</span>
            </div>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for BindPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let state = rsjs::shared::<BindEmitState>();
        let narrow = state.narrow();
        let name = state.name();
        let flag = state.flag();

        proa_macros::html_sync! {
            <form>
                <input type="number" bind_value={narrow} />
                <input type="text" bind_value={name} />
                <input type="checkbox" bind_checked={flag} />
            </form>
        }
        .render(cx)
    }
}

#[rsjs::rsjs(component, client)]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for PreludeOrderPanel {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<PreludeOrderEmitState>();
        let button_bg = workbench.button_bg();

        proa_macros::html_sync! {
            <div on_click={|_| button_bg.set("clicked".to_string())}>
                <h1>"Theme: "{button_bg.get()}</h1>
                <output>{button_bg.get()}</output>
            </div>
        }
        .render(cx)
    }
}

fn render(component: impl WebRenderSync<NoLoader, Vec<u8>>) -> String {
    let mut cx = Ctx::<NoLoader, Vec<u8>>::with_buffer(Vec::new());
    component.render(&mut cx).expect("island renders");
    String::from_utf8(cx.into_output()).expect("utf-8 html")
}

#[test]
fn field_and_index_projections_on_shared_signals_render_server_side() {
    let html = render(rsjs::SharedStateProvider {
        state: ProjectionEmitState {
            count: 1,
            row: EmitRow {
                total: 9,
                label: "hello".into(),
            },
            totals: vec![42, 43],
        },
        children: ProjectionPanel,
    });

    assert!(html.contains(">1</span>"), "{html}");
    assert!(html.contains(">hello</span>"), "{html}");
    assert!(html.contains(">42</span>"), "{html}");
}

#[test]
fn bound_inputs_render_the_seeded_shared_values() {
    let html = render(rsjs::SharedStateProvider {
        state: BindEmitState {
            narrow: 7,
            name: "bob".into(),
            flag: true,
        },
        children: BindPanel,
    });

    assert!(html.contains("value=\"7\""), "{html}");
    assert!(html.contains("value=\"bob\""), "{html}");
    assert!(html.contains("checked"), "{html}");
    assert!(!html.contains("value=\"\""), "{html}");
}

#[test]
fn a_shared_field_is_available_to_later_template_lowering_and_ssr() {
    let html = render(rsjs::SharedStateProvider {
        state: PreludeOrderEmitState {
            button_bg: "green".into(),
        },
        children: PreludeOrderPanel,
    });

    assert!(html.contains(">Theme: <span"), "{html}");
    assert!(html.contains(">green</span></h1>"), "{html}");
    assert!(html.contains(">green</output>"), "{html}");
}
