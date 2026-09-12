// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::loader::{database_status_key, DatabaseStatus};

#[derive(Debug, Clone, Copy)]
pub struct DatabaseCard;

impl<L: DataLoader> WebRender<L> for DatabaseCard {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx
                .load_typed::<DatabaseStatus>(database_status_key())
                .await?;
            WebRenderSync::render(
                html_sync! {
                    <article data-database-card class="metric-card">
                        <p class="metric-label">"DATA ORIGIN"</p>
                        <p class="metric-value">{status.driver}</p>
                        <p class="metric-note">
                            "Two component requests resolve through one keyed origin query."
                        </p>
                    </article>
                },
                cx,
            )
        })
    }
}
