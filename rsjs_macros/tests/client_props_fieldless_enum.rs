// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end fieldless-enum transport through a real rsjs hydration seed.

use proa_core::{render_to_string, DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
#[allow(unused_imports)]
use rsjs::{console, event_handler, rsjs, signal};

#[derive(Debug, PartialEq, rsjs::ClientProps)]
enum FulfillmentMode {
    Pickup,
    Delivery,
}

struct FulfillmentPicker {
    initial: FulfillmentMode,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for FulfillmentPicker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mode = signal(self.initial);
        let inspect = event_handler(|_: rsjs::Event| {
            console::log(mode.get());
        });

        proa_macros::html_sync! {
            <button type="button" data-mode-picker on_click={inspect()}>"Inspect mode"</button>
        }
        .render(cx)
    }
}

#[test]
fn fieldless_enum_crosses_as_a_typed_tagged_string_seed() {
    let html = render_to_string(FulfillmentPicker {
        initial: FulfillmentMode::Delivery,
    });

    assert!(
        html.contains(r#"{"signals":["Delivery"]}"#),
        "the generated hydration seed must carry the enum's stable string tag: {html}"
    );
    assert!(
        html.contains(r#"data-rsjs-handler="0""#),
        "the browser-reaching enum read must keep the island hydrated: {html}"
    );
}

#[test]
fn all_fieldless_variants_use_the_same_seed_schema() {
    let html = render_to_string(FulfillmentPicker {
        initial: FulfillmentMode::Pickup,
    });

    assert!(
        html.contains(r#"{"signals":["Pickup"]}"#),
        "the alternate enum variant must use its declared tag: {html}"
    );
}
