// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Streaming SSR primitives layered on top of `proa_core`.
//!
//! # Features
//!
//! - `rt-tokio` (default): spawn prepared loads eagerly on the ambient
//!   tokio runtime so they run in parallel with rendering. Disable it for
//!   runtime-agnostic builds: loads then start lazily on first await,
//!   overlap through cooperative polling, and the crate carries no runtime
//!   dependency at all.
//! - Artifact-aware out-of-order drains keep committed boundary HTML together
//!   with its rsjs island ids, query seeds, and SharedState seeds so framework
//!   adapters can emit metadata immediately before each swap frame.

pub mod context;
pub mod discovery;
pub mod driver;
pub mod handle;
pub mod loader_impls;
pub mod preloaded_loader;
pub mod request_id;
pub mod resolver;
pub mod runtime_script;
pub mod suspense;

pub use context::{SendStreamContext, StreamContext};
pub use discovery::CollectLoadKeys;
pub use driver::{
    drain_pending_boundaries_with_artifacts, drain_pending_boundaries_with_nonce_and_artifacts,
    render_md_in_order_to_flusher, render_md_in_order_to_vec, InOrderMarkdownDriver,
    OutOfOrderBoundaryChunk,
};
pub use handle::StreamHandle;
pub use loader_impls::{AxumLoader, FfiLoader};
pub use preloaded_loader::PreparedLoader;
pub use request_id::RequestId;
pub use resolver::DataResolver;
pub use runtime_script::{inline_runtime_tag, write_inline_runtime_tag};
pub use suspense::Suspense;
