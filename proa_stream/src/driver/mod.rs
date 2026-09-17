// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Streaming drivers for the different render modes.

pub mod in_order_markdown;
pub mod out_of_order;
#[cfg(feature = "rt-tokio")]
pub mod supervised;

pub use in_order_markdown::{
    render_md_in_order_to_flusher, render_md_in_order_to_flusher_with_loader,
    render_md_in_order_to_vec, render_md_in_order_to_vec_with_loader, InOrderMarkdownDriver,
};
pub use out_of_order::{
    drain_pending_boundaries_with_artifacts, drain_pending_boundaries_with_nonce_and_artifacts,
    OutOfOrderBoundaryChunk, SendOutOfOrderBoundaryDriver,
};
#[cfg(feature = "rt-tokio")]
pub use supervised::{
    LocalBoundarySupervisionGuard, RuntimeOutOfOrderBoundaryDriver,
    RuntimeSendOutOfOrderBoundaryDriver,
};
