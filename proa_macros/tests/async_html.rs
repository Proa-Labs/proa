// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{text, NoLoader, WebRender};
use proa_macros::html;

fn assert_async_renderable<R: WebRender<NoLoader>>(_renderable: R) {}

#[test]
fn static_async_html_implements_web_render() {
    assert_async_renderable(html! {
        <p>"Static"</p>
    });
}

#[test]
fn mixed_async_html_implements_web_render() {
    let label = "dynamic";

    assert_async_renderable(html! {
        <p>"Hello "{text(label)}</p>
    });
}
