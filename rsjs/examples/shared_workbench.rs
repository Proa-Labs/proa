// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed page-scoped state shared by two independent rsjs island roots.
//!
//! Framework HTML response paths (proa_framework_core) append the
//! shared-state seed table script to the page automatically. This example
//! embeds by hand, so it must emit the script itself from
//! `WebContext::rsjs_shared_state_seed_table_json`; without it, every
//! consuming island fails hydration with `rsjs/shared-state-missing-scope`.

use proa_core::{Ctx, NoLoader, TwoSync, WebContext, WebRenderSync, WriteBuf, WriteError};

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "example.workbench")]
struct WorkbenchState {
    mode: String,
    button_bg: String,
}

struct CanvasToolbar;

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CanvasToolbar {
    // proa-perf-ok(scope): owned String writes are lowered to browser event-handler code, not SSR allocations
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();

        proa_macros::html_sync! {
            <button on_click={|_| rsjs::batch(|| {
                workbench.mode().set("edit".to_string());
                workbench.button_bg().set("#2f6b4f".to_string());
            })}>
                "Edit in green"
            </button>
        }
        .render(cx)
    }
}

struct CanvasPreview;

#[rsjs::rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for CanvasPreview {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let label = rsjs::memo(|| workbench.button_bg().get());

        proa_macros::html_sync! {
            <output>{label.get()}</output>
        }
        .render(cx)
    }
}

// proa-perf-ok(scope): standalone example manually embeds the framework's trusted, script-safe shared-state seed table
fn main() {
    let page = rsjs::SharedStateProvider {
        state: WorkbenchState {
            mode: "select".into(),
            button_bg: "#3366cc".into(),
        },
        children: TwoSync {
            first: CanvasToolbar,
            second: CanvasPreview,
        },
    };

    let mut out = Vec::new();
    let seed_table_json = {
        let mut cx = Ctx::<NoLoader, &mut Vec<u8>>::with_buffer(&mut out);
        page.render(&mut cx).expect("workbench page renders");
        cx.rsjs_shared_state_seed_table_json()
            .expect("the provider registered a shared-state scope")
    };

    let mut html = String::from_utf8(out).expect("rendered HTML is UTF-8");
    // The framework response paths append this script automatically; a page
    // assembled by hand must include it or islands cannot hydrate.
    html.push_str(r#"<script type="application/json" id="rsjs-shared-state-seed-table">"#);
    html.push_str(&seed_table_json);
    html.push_str("</script>");

    assert!(
        html.contains(r#"id="rsjs-shared-state-seed-table""#),
        "manual embedding must include the shared-state seed table script",
    );

    println!("{html}");
}
