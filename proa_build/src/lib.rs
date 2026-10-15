// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Linked RSJS compilation, artifact caching, and semantic linking.
//!
//! Kept independent of CLI commands and source editing tools so each domain
//! can reuse the other domains when contributors edit it.

pub mod abi_adapter;
pub mod build;
pub mod cache;
pub mod final_compile;
pub(crate) mod legacy_graph;
pub mod link;
pub mod pipeline;
pub mod semantic_link;

mod descriptor_objects;
pub(crate) mod timing;
