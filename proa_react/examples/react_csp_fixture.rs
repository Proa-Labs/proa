// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{ctx::Ctx, CspNonce, Render};
use proa_macros::html_sync;
use proa_react::{
    react_bootstrap, react_island, render_react_import_map, HydrationStrategy,
    ReactBootstrapConfig, ReactModuleDescriptor,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let nonce = std::env::args()
        .nth(1)
        .ok_or("usage: react_csp_fixture <base64-nonce>")?;
    let nonce = CspNonce::new(nonce)?;
    let config = ReactBootstrapConfig {
        react_module_url: "/static/react.js",
        react_dom_module_url: "/static/react-dom-client.js",
        ..ReactBootstrapConfig::default()
    };

    let mut cx = Ctx::new().with_csp_nonce(nonce);

    react_island("Counter", "counter-island", r#"{"count":1}"#, |cx| {
        html_sync! {
            <button id="counter-button" type="button">"Count: 1"</button>
        }
        .render(cx)
    })
    .render(&mut cx)?;

    render_react_import_map(&mut cx, &config)?;
    react_bootstrap(
        &[ReactModuleDescriptor {
            island: "Counter",
            module_path: "/static/counter.js",
            export: None,
            strategy: HydrationStrategy::Immediate,
        }],
        &config,
    )
    .render(&mut cx)?;

    let body = cx.into_output().into_string();
    print!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Proa React CSP smoke</title></head><body>{body}</body></html>"
    );
    Ok(())
}
