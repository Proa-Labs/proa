// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use proa_core::{
    render_in_send_context, RenderOutcome, SendWebContext, WebRenderSend, WriteBuf, WriteError,
};
use proa_macros::html_send;

struct PendingChild(&'static str);

impl WebRenderSend for PendingChild {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            cx.out.extend(self.0.as_bytes())
        })
    }
}

#[tokio::test]
async fn send_html_future_can_run_in_a_spawned_task() {
    let label = String::from("ready & safe");
    let rendered = tokio::spawn(async move {
        let root = html_send! {
            <main data-label={label}>
                {PendingChild("done")}
            </main>
        };
        let mut cx = SendWebContext::new();
        render_in_send_context(&mut cx, root).await.unwrap();
        String::from_utf8(cx.into_output().into_vec()).unwrap()
    })
    .await
    .unwrap();

    assert_eq!(
        rendered,
        r#"<main data-label="ready &amp; safe">done</main>"#
    );
}
